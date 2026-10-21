//! The real display: KMS through DRM, buffers from GBM, rendering with
//! EGL/GLES, input from libinput, and device access through libseat (no
//! root needed when seatd or logind runs). Modelled on Smithay's anvil
//! (v0.7.0, MIT licence), reduced to one GPU.
//!
//! Each connected display gets its own render loop: render, queue the
//! frame, wait for its vblank, render again. When nothing changed, the
//! display goes idle until a client commits or input arrives, and renders
//! at once then (not at the next timer tick: that cost refreshes).

use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::{Rc, Weak},
    time::Duration,
};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmEvent, DrmEventTime, DrmNode, NodeType,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
            gles::GlesRenderer,
        },
        session::{
            Event as SessionEvent, Session,
            libseat::{LibSeatSession, LibSeatSessionNotifier},
        },
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    output::{Mode as WlMode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            EventLoop, LoopHandle,
            ping::make_ping,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as _, ModeTypeFlags, connector, crtc},
        input::Libinput,
        rustix::fs::OFlags,
    },
    utils::{DeviceFd, Logical, Point, Rectangle, Transform},
    wayland::{
        dmabuf::DmabufFeedbackBuilder,
        drm_syncobj::{DrmSyncobjState, supports_syncobj_eventfd},
    },
};

use crate::{
    Comp, LoopData,
    backend::BACKGROUND,
    cursor,
    render::{self, OutputElement, Shaders},
    state::SessionRequest,
    stats::FrameStats,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Manager = DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>;
type Out = DrmOutput<Allocator, Exporter, (), DrmDeviceFd>;

/// Scan-out formats we ask for, best first.
const FORMATS: [Fourcc; 2] = [Fourcc::Abgr8888, Fourcc::Argb8888];

/// A display's render loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopState {
    /// Not rendering (the session is paused, or the display is gone).
    Stopped,
    /// A render is scheduled.
    Scheduled,
    /// A frame is queued and waits for its vblank.
    Queued,
    /// The last render had nothing new: the next one waits for a change
    /// (a client commit or input wakes it, see `Comp::request_redraw`).
    Idle,
}

/// One connected display.
struct Screen {
    output: Output,
    connector: connector::Handle,
    drm: Out,
    refresh: Duration,
    /// Where its render loop is.
    state: LoopState,
    /// Frame callbacks are scheduled for a refresh period after an empty
    /// frame (so clients asking only for callbacks cannot spin the loop).
    callbacks_pending: bool,
    /// Frame timing, when `FERN_COMP_STATS` is set.
    stats: Option<FrameStats>,
}

struct Drm {
    me: Weak<RefCell<Drm>>,
    handle: LoopHandle<'static, LoopData>,
    session: LibSeatSession,
    node: DrmNode,
    manager: Manager,
    renderer: GlesRenderer,
    shaders: Shaders,
    wallpapers: render::Wallpapers,
    pointer: MemoryRenderBuffer,
    screens: HashMap<crtc::Handle, Screen>,
    libinput: Libinput,
    /// We hold the seat (false while another VT is shown).
    active: bool,
    /// The colour temperature the displays' gamma ramps show (None: not
    /// set since a display appeared or we came back from another VT).
    gamma: Option<u32>,
}

/// Takes over the display.
///
/// # Errors
/// No session (seatd/logind not running and not root), no GPU, or the GPU
/// cannot be set up for KMS and EGL.
pub fn init(
    event_loop: &mut EventLoop<'static, LoopData>,
    data: &mut LoopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut session, notifier) =
        LibSeatSession::new().map_err(|e| format!("session (is seatd running?): {e}"))?;
    let seat = session.seat();
    let path = match std::env::var_os("FERN_DRM_DEVICE") {
        Some(p) => PathBuf::from(p),
        None => pick_gpu(
            primary_gpu(&seat).ok().flatten(),
            all_gpus(&seat).unwrap_or_default(),
            |p| driver_of(p).as_deref().is_some_and(is_firmware_framebuffer),
        )
        .ok_or("no GPU found")?,
    };
    let node = DrmNode::from_path(&path)?;
    tracing::info!(gpu = %path.display(), "opening");
    let (manager, mut renderer, drm_events) = open_gpu(&mut session, &path, node)?;
    let mut shaders = Shaders::compile(&mut renderer)?;
    let software = EGLDevice::device_for_display(renderer.egl_context().display())
        .is_ok_and(|d| d.is_software());
    shaders.blur_on = render::blur_wanted(!software);
    if !shaders.blur_on {
        tracing::info!("blur off (software rendering; FERN_COMP_BLUR=1 turns it on)");
    }

    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| format!("libinput: cannot use seat {seat}"))?;

    let pointer = MemoryRenderBuffer::from_slice(
        &cursor::image(1),
        Fourcc::Argb8888,
        (cursor::SIZE, cursor::SIZE),
        1,
        Transform::Normal,
        None,
    );

    let drm = Rc::new_cyclic(|me| {
        RefCell::new(Drm {
            me: me.clone(),
            handle: event_loop.handle(),
            session,
            node,
            manager,
            renderer,
            shaders,
            wallpapers: render::Wallpapers::new(render::default_wallpaper()),
            pointer,
            screens: HashMap::new(),
            libinput: libinput.clone(),
            active: true,
            gamma: None,
        })
    });

    register_sources(
        &event_loop.handle(),
        &drm,
        libinput,
        notifier,
        drm_events,
        &seat,
    )?;
    gpu_clients(&drm, node, &mut data.state);
    let (ping, wakeups) = make_ping()?;
    data.state.redraw = Some(ping);
    let d = Rc::clone(&drm);
    event_loop
        .handle()
        .insert_source(wakeups, move |(), &mut (), data| {
            if let Ok(mut drm) = d.try_borrow_mut() {
                drm.wake(&mut data.state);
            }
        })
        .map_err(|e| format!("event loop: {}", e.error))?;

    drm.borrow_mut().scan(&mut data.state);
    if drm.borrow().screens.is_empty() {
        return Err("no connected display".into());
    }
    // The loop's sources own the backend from here on.
    Ok(())
}

/// Lets clients render on the GPU: linux-dmabuf with the renderer's
/// formats (clients render on the GPU's render node), every buffer
/// checked by importing it, and explicit sync when the kernel driver
/// supports it.
fn gpu_clients(drm: &Rc<RefCell<Drm>>, node: DrmNode, state: &mut Comp) {
    let render_node = node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .unwrap_or(node);
    let (formats, fd) = {
        let d = drm.borrow();
        (
            d.renderer.dmabuf_formats(),
            d.manager.device().device_fd().clone(),
        )
    };
    match DmabufFeedbackBuilder::new(render_node.dev_id(), formats).build() {
        Ok(feedback) => {
            let dh = state.display_handle.clone();
            state.dmabuf_global = Some(
                state
                    .dmabuf_state
                    .create_global_with_default_feedback::<Comp>(&dh, &feedback),
            );
            let weak = Rc::downgrade(drm);
            state.dmabuf_import = Some(Box::new(move |dmabuf| {
                weak.upgrade().is_some_and(|drm| {
                    drm.try_borrow_mut()
                        .is_ok_and(|mut drm| drm.renderer.import_dmabuf(dmabuf, None).is_ok())
                })
            }));
        }
        Err(e) => tracing::warn!("no GPU buffers for clients: {e}"),
    }
    if supports_syncobj_eventfd(&fd) {
        state.syncobj_state = Some(DrmSyncobjState::new::<Comp>(&state.display_handle, fd));
    } else {
        tracing::info!("no explicit sync (the kernel driver does not support it)");
    }
}

/// Connects the backend to the event loop: input, session changes,
/// vblanks and display hotplug.
fn register_sources(
    handle: &LoopHandle<'static, LoopData>,
    drm: &Rc<RefCell<Drm>>,
    libinput: Libinput,
    notifier: LibSeatSessionNotifier,
    drm_events: DrmDeviceNotifier,
    seat: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let d = Rc::clone(drm);
    handle
        .insert_source(
            LibinputInputBackend::new(libinput),
            move |event, (), data| {
                data.state.process_input_event(event);
                if let Some(SessionRequest::SwitchVt(vt)) = data.state.session_request.take()
                    && let Ok(mut drm) = d.try_borrow_mut()
                    && let Err(e) = drm.session.change_vt(vt)
                {
                    tracing::warn!("switching to VT {vt}: {e}");
                }
            },
        )
        .map_err(|e| format!("event loop: {}", e.error))?;

    let d = Rc::clone(drm);
    handle
        .insert_source(notifier, move |event, (), data| {
            if let Ok(mut drm) = d.try_borrow_mut() {
                match event {
                    SessionEvent::PauseSession => drm.pause(),
                    SessionEvent::ActivateSession => drm.activate(&mut data.state),
                }
            }
        })
        .map_err(|e| format!("event loop: {}", e.error))?;

    let d = Rc::clone(drm);
    handle
        .insert_source(drm_events, move |event, metadata, data| match event {
            DrmEvent::VBlank(crtc) => {
                if let Ok(mut drm) = d.try_borrow_mut() {
                    let at = match metadata.as_ref().map(|m| m.time) {
                        Some(DrmEventTime::Monotonic(t)) => Some(t),
                        Some(DrmEventTime::Realtime(t)) => {
                            t.duration_since(std::time::UNIX_EPOCH).ok()
                        }
                        None => None,
                    };
                    drm.vblank(crtc, at, &mut data.state);
                }
            }
            DrmEvent::Error(e) => tracing::warn!("drm: {e}"),
        })
        .map_err(|e| format!("event loop: {}", e.error))?;

    // Displays plugged in or out.
    let d = Rc::clone(drm);
    handle
        .insert_source(UdevBackend::new(seat)?, move |event, (), data| {
            if let UdevEvent::Changed { device_id } = event
                && let Ok(mut drm) = d.try_borrow_mut()
                && drm.node.dev_id() == device_id
            {
                drm.scan(&mut data.state);
            }
        })
        .map_err(|e| format!("event loop: {}", e.error))?;
    Ok(())
}

/// The GPU to drive: the boot GPU, unless it is the firmware's framebuffer
/// (simpledrm, which stays around next to the real driver on some
/// machines and in QEMU, but nothing shows it once the real driver runs);
/// then any real GPU; the firmware framebuffer only when there is nothing
/// else.
fn pick_gpu(
    primary: Option<PathBuf>,
    all: Vec<PathBuf>,
    firmware: impl Fn(&std::path::Path) -> bool,
) -> Option<PathBuf> {
    primary
        .clone()
        .filter(|p| !firmware(p))
        .or_else(|| all.iter().find(|p| !firmware(p)).cloned())
        .or(primary)
        .or_else(|| all.into_iter().next())
}

/// Kernel drivers of firmware-provided framebuffers.
fn is_firmware_framebuffer(driver: &str) -> bool {
    matches!(
        driver,
        "simple-framebuffer" | "simpledrm" | "efi-framebuffer" | "vesa-framebuffer"
    )
}

/// The kernel driver behind a DRM device node (`/dev/dri/cardN`).
fn driver_of(path: &std::path::Path) -> Option<String> {
    let name = path.file_name()?;
    let link = std::fs::read_link(
        std::path::Path::new("/sys/class/drm")
            .join(name)
            .join("device/driver"),
    )
    .ok()?;
    Some(link.file_name()?.to_string_lossy().into_owned())
}

/// Opens the GPU for KMS and sets up EGL rendering on it.
fn open_gpu(
    session: &mut LibSeatSession,
    path: &std::path::Path,
    node: DrmNode,
) -> Result<(Manager, GlesRenderer, DrmDeviceNotifier), Box<dyn std::error::Error>> {
    let fd = session.open(
        path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (device, drm_events) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    // SAFETY: the GBM device lives as long as the display (EGL keeps a
    // reference to it), and it is a valid GBM device.
    #[allow(unsafe_code)]
    let display = unsafe { EGLDisplay::new(gbm.clone())? };
    if EGLDevice::device_for_display(&display).is_ok_and(|d| d.is_software()) {
        tracing::warn!("no GPU acceleration: rendering in software");
    }
    let context = EGLContext::new(&display)?;
    // SAFETY: the context was just created and is only used here.
    #[allow(unsafe_code)]
    let renderer = unsafe { GlesRenderer::new(context)? };
    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), Some(node));
    let manager = DrmOutputManager::new(
        device,
        allocator,
        exporter,
        Some(gbm),
        FORMATS,
        render_formats,
    );
    Ok((manager, renderer, drm_events))
}

impl Drm {
    /// Brings `screens` in line with the connectors: new displays get an
    /// output and a render loop; unplugged ones are removed.
    fn scan(&mut self, state: &mut Comp) {
        let device = self.manager.device();
        let Ok(res) = device.resource_handles() else {
            tracing::warn!("cannot read the GPU's connectors");
            return;
        };
        let gone: Vec<crtc::Handle> = self
            .screens
            .iter()
            .filter(|(_, s)| {
                device
                    .get_connector(s.connector, false)
                    .map_or(true, |c| c.state() != connector::State::Connected)
            })
            .map(|(c, _)| *c)
            .collect();
        let mut taken: Vec<crtc::Handle> = self.screens.keys().copied().collect();
        let mut new = Vec::new();
        for &conn in res.connectors() {
            if self.screens.values().any(|s| s.connector == conn) {
                continue;
            }
            let Ok(info) = device.get_connector(conn, false) else {
                continue;
            };
            if info.state() != connector::State::Connected || info.modes().is_empty() {
                continue;
            }
            let crtc = info
                .encoders()
                .iter()
                .filter_map(|e| device.get_encoder(*e).ok())
                .flat_map(|e| res.filter_crtcs(e.possible_crtcs()))
                .find(|c| !taken.contains(c));
            if let Some(crtc) = crtc {
                taken.push(crtc);
                new.push((info, crtc));
            } else {
                tracing::warn!(?conn, "no free CRTC for a connected display");
            }
        }
        for crtc in gone {
            if let Some(s) = self.screens.remove(&crtc) {
                tracing::info!(output = s.output.name(), "display removed");
                state.space.unmap_output(&s.output);
            }
        }
        for (info, crtc) in new {
            if let Err(e) = self.connect(&info, crtc, state) {
                tracing::warn!("setting up a display: {e}");
            }
        }
    }

    fn connect(
        &mut self,
        info: &connector::Info,
        crtc: crtc::Handle,
        state: &mut Comp,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let modes = info.modes();
        let mode = modes
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .unwrap_or(&modes[0]);
        let wl_mode = WlMode::from(*mode);
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let (w, h) = info.size().unwrap_or((0, 0));
        let output = Output::new(
            name,
            PhysicalProperties {
                size: (i32::try_from(w)?, i32::try_from(h)?).into(),
                subpixel: Subpixel::Unknown,
                make: "Unknown".into(),
                model: "Unknown".into(),
            },
        );
        let _global = output.create_global::<Comp>(&state.display_handle);
        // Side by side, left to right, in the order they appear.
        let x = state
            .space
            .outputs()
            .filter_map(|o| state.space.output_geometry(o))
            .map(|g| g.loc.x + g.size.w)
            .max()
            .unwrap_or(0);
        output.set_preferred(wl_mode);
        output.change_current_state(
            Some(wl_mode),
            Some(Transform::Normal),
            None,
            Some((x, 0).into()),
        );
        state.space.map_output(&output, (x, 0));
        state.output_changed(&output);
        let drm = self
            .manager
            .initialize_output::<_, OutputElement>(
                crtc,
                *mode,
                &[info.handle()],
                &output,
                None,
                &mut self.renderer,
                &DrmOutputRenderElements::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        let millihertz = u64::try_from(wl_mode.refresh).unwrap_or(60_000).max(1_000);
        tracing::info!(
            output = output.name(),
            size = ?wl_mode.size,
            hz = millihertz / 1000,
            "display ready"
        );
        self.gamma = None;
        self.screens.insert(
            crtc,
            Screen {
                output,
                connector: info.handle(),
                drm,
                refresh: Duration::from_micros(1_000_000_000 / millihertz),
                state: LoopState::Stopped,
                callbacks_pending: false,
                stats: std::env::var_os("FERN_COMP_STATS")
                    .map(|_| FrameStats::new(Duration::from_micros(1_000_000_000 / millihertz))),
            },
        );
        self.start(crtc);
        Ok(())
    }

    /// Starts a display's render loop if it is not running.
    fn start(&mut self, crtc: crtc::Handle) {
        if let Some(s) = self.screens.get_mut(&crtc)
            && s.state == LoopState::Stopped
        {
            s.state = LoopState::Scheduled;
            self.after(Duration::ZERO, crtc);
        }
    }

    /// Night Shift: every display's gamma ramps for `kelvin` (0 neutral).
    /// A display without ramps stays as it is.
    fn apply_gamma(&mut self, kelvin: u32) {
        let device = self.manager.device();
        for crtc in self.screens.keys() {
            let size = device.get_crtc(*crtc).map_or(0, |c| c.gamma_length());
            if size == 0 {
                continue;
            }
            let [r, g, b] = crate::nightshift::ramps(kelvin, size);
            if let Err(e) = device.set_gamma(*crtc, &r, &g, &b) {
                tracing::warn!(?crtc, "gamma ramps: {e}");
            }
        }
        self.gamma = Some(kelvin);
    }

    /// Renders `crtc` after `delay`.
    fn after(&self, delay: Duration, crtc: crtc::Handle) {
        let me = self.me.clone();
        let timer = self
            .handle
            .insert_source(Timer::from_duration(delay), move |_, (), data| {
                if let Some(drm) = me.upgrade()
                    && let Ok(mut drm) = drm.try_borrow_mut()
                {
                    drm.render(crtc, &mut data.state);
                }
                TimeoutAction::Drop
            });
        if let Err(e) = timer {
            tracing::warn!("frame timer: {}", e.error);
        }
    }

    /// Wakes idle displays: something changed (a commit, input).
    fn wake(&mut self, state: &mut Comp) {
        let idle: Vec<crtc::Handle> = self
            .screens
            .iter()
            .filter(|(_, s)| s.state == LoopState::Idle)
            .map(|(c, _)| *c)
            .collect();
        for crtc in idle {
            self.render(crtc, state);
        }
    }

    /// One turn of a display's loop: render, and queue the frame if
    /// anything changed; otherwise wait for a change.
    fn render(&mut self, crtc: crtc::Handle, state: &mut Comp) {
        if self.active && self.gamma != Some(state.color_temperature) {
            self.apply_gamma(state.color_temperature);
        }
        let Some(screen) = self.screens.get_mut(&crtc) else {
            return;
        };
        if !self.active {
            screen.state = LoopState::Stopped;
            return;
        }
        if screen.state == LoopState::Queued {
            // The vblank renders next.
            return;
        }
        let Some(area) = state.space.output_geometry(&screen.output) else {
            screen.state = LoopState::Stopped;
            return;
        };
        if let Some(req) = state.wallpaper_request.take() {
            self.wallpapers.change(req.as_deref());
        }
        let animating = state.advance_animations() | self.wallpapers.tick();
        let mut elements: Vec<OutputElement> = Vec::new();
        if let Some(pointer) = state.seat.get_pointer() {
            let at = pointer.current_location().to_i32_round();
            if area.contains(at) {
                let loc: Point<i32, Logical> = at - area.loc - Point::from(cursor::HOTSPOT);
                if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
                    &mut self.renderer,
                    loc.to_f64().to_physical(1.0),
                    &self.pointer,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    elements.push(OutputElement::Image(e));
                }
            }
        }
        let pointer = elements.len();
        elements.extend(render::output_elements(
            &mut self.renderer,
            &self.shaders,
            &state.space,
            Some(&screen.output),
            &self.wallpapers,
            Rectangle::new(area.loc, area.size),
            1.0,
            &|w| state.visual(w),
            &state.overlay(),
        ));
        let queued = match screen.drm.render_frame(
            &mut self.renderer,
            &elements,
            BACKGROUND,
            FrameFlags::DEFAULT,
        ) {
            Ok(frame) if !frame.is_empty => match screen.drm.queue_frame(()) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("queueing a frame: {e:?}");
                    false
                }
            },
            Ok(_) => false,
            Err(e) => {
                tracing::warn!("rendering: {e:?}");
                false
            }
        };
        // Screenshots: what was just drawn, without the pointer; after the
        // frame, so the frame's elements are drawn for the display first
        // (some keep state from one frame to the next).
        let shots = state.take_screenshots(area);
        crate::screenshot::answer(&mut self.renderer, &elements[pointer..], area, shots);
        screen.state = if queued {
            LoopState::Queued
        } else {
            LoopState::Idle
        };
        let refresh = screen.refresh;
        if !queued && !screen.callbacks_pending {
            // Nothing new on screen. Clients waiting for a frame callback
            // get it a refresh period later, as if a frame had been shown.
            screen.callbacks_pending = true;
            self.callbacks_after(refresh, crtc);
        }
        if !queued && animating {
            // A spring moved less than a pixel: look again next refresh.
            self.after(refresh, crtc);
        }
    }

    /// Sends the frame callbacks of an idle display after `delay`.
    fn callbacks_after(&self, delay: Duration, crtc: crtc::Handle) {
        let me = self.me.clone();
        let timer = self
            .handle
            .insert_source(Timer::from_duration(delay), move |_, (), data| {
                if let Some(drm) = me.upgrade()
                    && let Ok(mut drm) = drm.try_borrow_mut()
                    && let Some(screen) = drm.screens.get_mut(&crtc)
                {
                    screen.callbacks_pending = false;
                    if screen.state == LoopState::Idle {
                        data.state.frame_shown(&screen.output);
                        let _ = data.state.display_handle.flush_clients();
                    }
                }
                TimeoutAction::Drop
            });
        if let Err(e) = timer {
            tracing::warn!("frame callback timer: {}", e.error);
        }
    }

    /// The display showed the queued frame.
    fn vblank(&mut self, crtc: crtc::Handle, at: Option<Duration>, state: &mut Comp) {
        let Some(screen) = self.screens.get_mut(&crtc) else {
            return;
        };
        screen.state = LoopState::Scheduled;
        if let (Some(stats), Some(at)) = (screen.stats.as_mut(), at)
            && let Some(report) = stats.presented(at)
        {
            tracing::info!(output = screen.output.name(), "{report}");
        }
        if let Err(e) = screen.drm.frame_submitted() {
            tracing::warn!("frame submitted: {e:?}");
        }
        state.frame_shown(&screen.output);
        let _ = state.display_handle.flush_clients();
        self.render(crtc, state);
    }

    /// Another VT took the display: stop touching the devices.
    fn pause(&mut self) {
        tracing::info!("session paused");
        self.active = false;
        // Every loop restarts with a full frame when the session returns.
        for screen in self.screens.values_mut() {
            screen.state = LoopState::Stopped;
            // Timing restarts too: a report never spans time away.
            if let Some(stats) = &mut screen.stats {
                *stats = FrameStats::new(screen.refresh);
            }
        }
        self.libinput.suspend();
        self.manager.pause();
    }

    /// The display is ours again.
    fn activate(&mut self, state: &mut Comp) {
        tracing::info!("session active");
        if let Err(e) = self.libinput.resume() {
            tracing::warn!("libinput: {e:?}");
        }
        if let Err(e) = self.manager.activate(false) {
            tracing::warn!("drm: {e}");
        }
        self.active = true;
        self.gamma = None;
        self.scan(state);
        let crtcs: Vec<crtc::Handle> = self.screens.keys().copied().collect();
        for crtc in crtcs {
            self.start(crtc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_gpus_win_over_the_firmware_framebuffer() {
        let p = |s: &str| PathBuf::from(s);
        let fw = |path: &std::path::Path| path.ends_with("card1");
        // QEMU: no boot-VGA GPU, simpledrm enumerated first.
        assert_eq!(
            pick_gpu(None, vec![p("card1"), p("card0")], fw),
            Some(p("card0"))
        );
        // The boot GPU is the firmware framebuffer: take a real one.
        assert_eq!(
            pick_gpu(Some(p("card1")), vec![p("card1"), p("card0")], fw),
            Some(p("card0"))
        );
        // A real boot GPU is kept even when others exist.
        assert_eq!(
            pick_gpu(Some(p("card2")), vec![p("card0"), p("card2")], fw),
            Some(p("card2"))
        );
        // Only the firmware framebuffer: better than nothing.
        assert_eq!(pick_gpu(None, vec![p("card1")], fw), Some(p("card1")));
        assert_eq!(pick_gpu(None, vec![], fw), None);
        assert!(
            is_firmware_framebuffer("simple-framebuffer") && !is_firmware_framebuffer("virtio-pci")
        );
    }
}
