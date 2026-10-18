# Sandbox probe

`probe/recipe.toml` is a deliberately hostile recipe: its build script tries
to reach the network, run as root, use capabilities, write to the toolchain
or sysroot, and see the host's files and processes. The build only succeeds
if every attempt fails. Run it with `just sandbox-probe`.

It lives outside `recipes/` because it is a test, not a system package.
