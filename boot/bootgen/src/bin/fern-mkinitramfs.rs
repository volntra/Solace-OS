//! fern-mkinitramfs: writes Fern's initramfs (fern-init as /init).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [init, out] = args.as_slice() else {
        eprintln!("usage: fern-mkinitramfs <fern-init binary> <output>");
        return ExitCode::from(2);
    };
    let run = || -> std::io::Result<()> {
        let data = std::fs::read(init)?;
        if !data.starts_with(b"\x7fELF") {
            return Err(std::io::Error::other(format!(
                "{init} is not an ELF binary"
            )));
        }
        let tmp = format!("{out}.tmp");
        bootgen::initramfs::write(std::io::BufWriter::new(std::fs::File::create(&tmp)?), &data)?;
        std::fs::rename(&tmp, out)
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fern-mkinitramfs: {e}");
            ExitCode::FAILURE
        }
    }
}
