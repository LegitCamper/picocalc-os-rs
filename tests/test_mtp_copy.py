#!/usr/bin/env python3
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


def fake_gio():
    arguments = sys.argv[2:]
    if arguments[:2] == ["--host", "gio"]:
        arguments = arguments[2:]
    operation, *values = arguments
    root = Path(os.environ["MTP_TEST_ROOT"])
    mode = os.environ.get("MTP_TEST_FAILURE", "")
    uri = "mtp://LegitCamper_PicoCalc_01001100"
    if operation == "mount":
        print("activation_root=mtp://Other_Device/")
        print("activation_root=" + uri)
        return
    if operation == "list":
        if mode == "list":
            sys.exit(1)
        assert values[0].startswith(uri)
        print("\n".join(sorted(path.name for path in root.iterdir())))
        return
    if operation == "cat":
        raise AssertionError("MTP backend does not support streaming reads")
    download = operation == "copy" and values[0].startswith(uri)
    remote = values[0] if download or operation == "remove" else values[-1]
    assert remote.startswith(uri + "/SD Card/")
    path = root / remote.rsplit("/", 1)[1]
    action = ("readback" if Path(values[-1]).name == "readback.bin" else "backup") if download else operation
    if mode == action and os.environ.get("MTP_TEST_FAILURE_NAME", path.name) == path.name:
        sys.exit(1)
    if download:
        Path(values[-1]).write_bytes(path.read_bytes())
    elif operation == "remove":
        path.unlink()
    elif operation == "copy":
        assert not path.exists()
        path.write_bytes(b"bad upload" if mode == "corrupt" else Path(values[0]).read_bytes())
    else:
        raise AssertionError(operation)


def check_copy_recipe():
    repository = Path(__file__).resolve().parents[1]
    justfile = (repository / "justfile").read_text()
    with tempfile.TemporaryDirectory(prefix="mtp-copy-tests-") as directory:
        work = Path(directory)
        (work / "justfile").write_text(justfile)
        tools = work / "bin"
        tools.mkdir()
        for tool in ["gio", "flatpak-spawn"]:
            launcher = tools / tool
            launcher.write_text(
                "#!/bin/sh\nexec " + shlex.quote(sys.executable) + " "
                + shlex.quote(str(Path(__file__).resolve())) + ' --gio "$@"\n'
            )
            launcher.chmod(0o755)
        root = work / "card"
        root.mkdir()
        temporary = work / "target"
        temporary.mkdir()
        binaries = work / "target/thumbv8m.main-none-eabihf/release-binary"
        binaries.mkdir(parents=True)
        environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                           MTP_TEST_ROOT=str(root), TMPDIR=str(temporary))

        def run(app, failure="", **extra):
            result = subprocess.run(["just", "copy-userapp", app], cwd=work,
                                    env=dict(environment, MTP_TEST_FAILURE=failure, **extra),
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            return result.returncode, result.stdout.decode()

        for app, name in [("gallery", "GALLERY.BIN"), ("calculator", "CALC.BIN"),
                          ("wav_player", "WAVPLAY.BIN")]:
            (binaries / app).write_bytes(b"new binary " + app.encode())
            for existing in [None, name, name.lower()]:
                for path in root.iterdir():
                    path.unlink()
                (root / "GIF.BIN").write_bytes(b"untouched neighbor")
                if existing:
                    (root / existing).write_bytes(b"old binary")
                code, output = run(app)
                assert code == 0, output
                assert (root / name).read_bytes() == (binaries / app).read_bytes()
                assert (root / "GIF.BIN").read_bytes() == b"untouched neighbor"
                assert len(list(root.iterdir())) == 2

        for failure in ["list", "backup", "remove", "copy", "readback", "corrupt"]:
            (root / "GALLERY.BIN").write_bytes(b"old binary")
            code, output = run("gallery", failure)
            assert code != 0, (failure, output)
            if failure in ["list", "backup", "remove"]:
                assert (root / "GALLERY.BIN").read_bytes() == b"old binary"
            if failure in ["copy", "readback", "corrupt"]:
                assert any(path.read_bytes() == b"old binary"
                           for path in temporary.glob("*/GALLERY.BIN"))

        assets = work / "assets"
        for app, source_name, card_name in [
            ("gallery", "Sprite-0001.bmp", "IMG0001.BMP"),
            ("gif", "bad_apple.gif", "BADAPPLE.GIF"),
            ("wav_player", "sample.wav", "SAMPLE.WAV"),
        ]:
            folder = assets / app
            folder.mkdir(parents=True)
            source = folder / source_name
            source.write_bytes(b"new asset " + app.encode())
            (binaries / app).write_bytes(b"new binary " + app.encode())
            (root / card_name.lower()).write_bytes(b"old asset")
            code, output = run(app)
            assert code == 0, output
            assert (root / card_name).read_bytes() == source.read_bytes()
            assert not (root / card_name.lower()).exists()
            code, output = run(app, "remove")
            assert code == 0, output
            assert "(unchanged)" in output

        (root / "IMG0001.BMP").write_bytes(b"old asset")
        code, output = run("gallery", "copy", MTP_TEST_FAILURE_NAME="IMG0001.BMP")
        assert code != 0, output
        assert any(path.read_bytes() == b"old asset"
                   for path in temporary.glob("*/IMG0001.BMP"))
        (assets / "gif/too-long-name.gif").write_bytes(b"invalid alias")
        assert run("gif")[0] != 0
        assert not (root / "TOO-LONG-NAME.GIF").exists()

        (root / "GALLERY.BIN").write_bytes(b"old binary")
        (binaries / "gallery").unlink()
        assert run("gallery")[0] != 0
        assert (root / "GALLERY.BIN").read_bytes() == b"old binary"
        assert run("../invalid")[0] != 0
    print("MTP copy recipe: apps, assets, aliases, backups, readback, and failures passed")


if __name__ == "__main__":
    if sys.argv[1:] and sys.argv[1] == "--gio":
        fake_gio()
    else:
        check_copy_recipe()
