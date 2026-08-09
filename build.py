import os, sys, argparse, subprocess, shutil
from platform import system

# ── Logger ───────────────────────────────────────────────────────────────────

if sys.stdout.encoding and sys.stdout.encoding.lower() != "utf-8":
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, OSError):
        pass

def _supports_color():
    return hasattr(sys.stdout, "isatty") and sys.stdout.isatty()

class C:
    RESET  = "\033[0m"        if _supports_color() else ""
    BOLD   = "\033[1m"        if _supports_color() else ""
    HEADER = "\033[38;5;111m" if _supports_color() else ""
    GREEN  = "\033[38;5;114m" if _supports_color() else ""
    YELLOW = "\033[38;5;221m" if _supports_color() else ""
    RED    = "\033[38;5;203m" if _supports_color() else ""
    DIM    = "\033[2m"        if _supports_color() else ""

def log_step(msg): print(f"\n{C.BOLD}{C.HEADER}==> {msg}{C.RESET}", flush=True)
def log_info(msg): print(f"    {C.DIM}{msg}{C.RESET}", flush=True)
def log_ok(msg):   print(f"    {C.GREEN}{msg}{C.RESET}", flush=True)
def log_error(msg):print(f"    {C.RED}[ERROR]{C.RESET} {msg}", flush=True)

# ── Platform ─────────────────────────────────────────────────────────────────

PLATFORM_TARGETS = {
    "Windows": "x86_64-pc-windows-msvc",
    "Linux":   "x86_64-unknown-linux-gnu",
    "Darwin":  "aarch64-apple-darwin",
}

PLATFORM_OUTPUTS = {
    "Windows": ("windows", "godot_wry.dll",       "vital.wry.{profile}.x86_64.dll"),
    "Linux":   ("linux",   "libgodot_wry.so",      "vital.wry.{profile}.x86_64.so"),
    "Darwin":  ("macos",   "libgodot_wry.dylib",   "vital.wry.{profile}.dylib"),
}

# ── Build ─────────────────────────────────────────────────────────────────────

class Build:
    def __init__(self, script_dir, build_type):
        self.script_dir = script_dir
        self.build_type = build_type
        self.os_type    = system()
        self.src_dir    = os.path.join(script_dir, "src")
        self.bin_dir    = os.path.join(script_dir, ".bin")
        self.build_dir  = os.path.join(script_dir, ".build")

    def compile(self):
        target = PLATFORM_TARGETS.get(self.os_type)
        if not target:
            log_error(f"Unsupported platform: {self.os_type}")
            sys.exit(1)

        log_step(f"Compiling [{self.os_type} | {self.build_type}]")
        log_info(f"Target: {target}")

        cmd = ["cargo", "build", "--target", target]
        if self.build_type == "release":
            cmd.append("--release")

        result = subprocess.run(cmd, cwd=self.src_dir)
        if result.returncode != 0:
            log_error("Cargo build failed")
            sys.exit(result.returncode)

        log_ok("Done")

    def stage(self):
        if self.os_type not in PLATFORM_OUTPUTS:
            log_error(f"Unsupported platform: {self.os_type}")
            sys.exit(1)

        target = PLATFORM_TARGETS[self.os_type]
        subdir, src_name, dst_template = PLATFORM_OUTPUTS[self.os_type]
        dst_name = dst_template.format(profile=self.build_type)

        src     = os.path.join(self.bin_dir, target, self.build_type, src_name)
        dst_dir = os.path.join(self.build_dir, subdir)
        dst     = os.path.join(dst_dir, dst_name)

        log_step(f"Staging [{self.os_type} | {self.build_type}]")
        log_info(f"{src_name} → .build/{subdir}/{dst_name}")

        if not os.path.exists(src):
            log_error(f"Expected output not found: {src}")
            sys.exit(1)

        os.makedirs(dst_dir, exist_ok=True)
        shutil.copy2(src, dst)
        log_ok("Done")


def main():
    parser = argparse.ArgumentParser(description="Build Vital.wry")

    build_group = parser.add_mutually_exclusive_group(required=True)
    build_group.add_argument("--debug",   action="store_true")
    build_group.add_argument("--release", action="store_true")
    build_group.add_argument("--all",     action="store_true", help="Build both release and debug")

    args = parser.parse_args()
    script_dir  = os.path.dirname(os.path.abspath(__file__))
    build_types = ["release", "debug"] if args.all else ["release"] if args.release else ["debug"]

    for build_type in build_types:
        b = Build(script_dir, build_type)
        b.compile()
        b.stage()

    log_step("Build complete")
    log_ok("Done")


if __name__ == "__main__":
    main()