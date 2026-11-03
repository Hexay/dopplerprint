import signal
import sys

from dopplerprint._core import cli


def main() -> int:
    signal.signal(signal.SIGINT, signal.SIG_DFL)  # the Rust CLI never returns to Python to see KeyboardInterrupt
    return cli(["dopplerprint", *sys.argv[1:]])


if __name__ == "__main__":
    sys.exit(main())
