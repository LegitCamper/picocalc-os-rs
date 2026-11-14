from pathlib import Path
import subprocess
import sys
import tempfile


tool = Path(__file__).resolve().parents[1] / "tools/mtp-smoke.py"
with tempfile.TemporaryDirectory() as directory:
    source = Path(directory) / "app.bin"
    source.write_bytes(b"app")
    empty = Path(directory) / "empty.bin"
    empty.touch()
    for args in [
        ["--replace", f"../GIF.BIN={source}"],
        ["--replace", f"BADAPPLE.GIF={source}"],
        ["--replace", "GIF.BIN"],
        ["--replace", f"GIF.BIN={empty}"],
        ["--replace", f"GIF.BIN={source}.missing"],
        ["--replace", f"GIF.BIN={source}", "--replace", f"gif.bin={source}"],
        ["--replace", f"GIF.BIN={source}", "--write-check"],
    ]:
        result = subprocess.run([sys.executable, str(tool), *args], capture_output=True, text=True)
        assert result.returncode == 2, result.stderr
        assert "error:" in result.stderr and "Traceback" not in result.stderr
print("MTP replacement rejects invalid requests before USB access")
