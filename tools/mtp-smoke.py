#!/usr/bin/env python3
"""PicoCalc MTP check. --write-check creates/removes unique 8.3 test files.

Unmount MTP first. Requires Python 3 and libusb; defaults to read-only.
"""

import argparse
import ctypes
import secrets
import struct


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-check", action="store_true")
    options = parser.parse_args()
    usb = ctypes.CDLL("libusb-1.0.so.0")
    usb.libusb_init.argtypes = [ctypes.c_void_p]
    usb.libusb_open_device_with_vid_pid.argtypes = [
        ctypes.c_void_p, ctypes.c_uint16, ctypes.c_uint16
    ]
    usb.libusb_open_device_with_vid_pid.restype = ctypes.c_void_p
    usb.libusb_claim_interface.argtypes = [ctypes.c_void_p, ctypes.c_int]
    usb.libusb_release_interface.argtypes = [ctypes.c_void_p, ctypes.c_int]
    usb.libusb_close.argtypes = [ctypes.c_void_p]
    usb.libusb_exit.argtypes = [ctypes.c_void_p]
    usb.libusb_bulk_transfer.argtypes = [
        ctypes.c_void_p, ctypes.c_ubyte, ctypes.c_void_p, ctypes.c_int,
        ctypes.POINTER(ctypes.c_int), ctypes.c_uint,
    ]
    usb.libusb_control_transfer.argtypes = [
        ctypes.c_void_p, ctypes.c_uint8, ctypes.c_uint8, ctypes.c_uint16,
        ctypes.c_uint16, ctypes.c_void_p, ctypes.c_uint16, ctypes.c_uint,
    ]
    assert usb.libusb_init(None) == 0
    device = usb.libusb_open_device_with_vid_pid(None, 0xC0DE, 0xBABE)
    assert device, "PicoCalc missing or USB access denied"
    claimed = False
    opened = False
    transaction = 0

    def transfer(endpoint, payload):
        size = len(payload) if isinstance(payload, bytes) else payload
        buffer = (ctypes.create_string_buffer(payload) if isinstance(payload, bytes)
                  else ctypes.create_string_buffer(size))
        actual = ctypes.c_int()
        status = usb.libusb_bulk_transfer(
            device, endpoint, buffer, size, ctypes.byref(actual), 5000
        )
        assert status == 0, f"USB endpoint {endpoint:#x}: {status}"
        if isinstance(payload, bytes):
            assert actual.value == size
        return buffer.raw[:actual.value]

    def container():
        data = transfer(0x81, 65536)
        while not data:
            data = transfer(0x81, 65536)
        assert len(data) >= 12, "Truncated container header"
        length, kind, code, tid = struct.unpack_from("<IHHI", data)
        assert 12 <= length <= 64 * 1024 * 1024, "Unreasonable container length"
        while len(data) < length:
            chunk = transfer(0x81, 65536)
            assert chunk, "Premature end of data"
            data += chunk
        assert len(data) == length, "Container length mismatch"
        return kind, code, tid, data[12:]

    def command(operation, *parameters, data=None, split=False, expected=0x2001):
        nonlocal transaction
        tid = transaction
        transaction += 1
        transfer(1, struct.pack("<IHHI", 12 + 4 * len(parameters), 1, operation, tid)
                 + b"".join(struct.pack("<I", value) for value in parameters))
        if data is not None:
            header = struct.pack("<IHHI", 12 + len(data), 2, operation, tid)
            if split and data:
                transfer(1, header)
                transfer(1, data)
                if len(data) % 64 == 0:
                    transfer(1, b"")
            else:
                transfer(1, header + data)
                if (12 + len(data)) % 64 == 0:
                    transfer(1, b"")
        kind, code, reply_tid, payload = container()
        assert reply_tid == tid, "Wrong transaction ID"
        dataset = None
        if kind == 2:
            assert code == operation, f"Data operation {code:#x} != {operation:#x}"
            dataset = payload
            kind, code, reply_tid, payload = container()
        assert kind == 3 and reply_tid == tid, "Invalid response"
        assert code == expected, f"Operation {operation:#x}: response {code:#x}, expected {expected:#x}"
        return dataset, payload

    def object_info(name, size):
        info = bytearray(52)
        struct.pack_into("<IHHI", info, 0, 0x00010001, 0x3000, 0, size)
        return bytes(info) + bytes([len(name) + 1]) + name.encode("utf-16le") + b"\0" * 5

    def descriptor(kind, index=0):
        buffer = ctypes.create_string_buffer(1024)
        length = usb.libusb_control_transfer(
            device, 0x80, 6, kind << 8 | index, 0x0409 if kind == 3 else 0,
            buffer, len(buffer), 5000,
        )
        assert length > 0, f"Descriptor {kind}:{index} failed: {length}"
        return buffer.raw[:length]

    try:
        configuration = descriptor(2)
        offset = 0
        interface = None
        endpoints = []
        while offset < len(configuration):
            length, kind = configuration[offset:offset + 2]
            assert length >= 2
            record = configuration[offset:offset + length]
            if kind == 4:
                interface = record
            elif kind == 5:
                endpoints.append((record[2], record[3] & 3))
            offset += length
        assert interface is not None and interface[4] == 3
        assert interface[5:8] == bytes([6, 1, 1])
        assert (1, 2) in endpoints and (0x81, 2) in endpoints
        assert any(address & 0x80 and kind == 3 for address, kind in endpoints)
        assert "MTP" in descriptor(3, interface[8])[2:].decode("utf-16le")
        assert usb.libusb_claim_interface(device, 0) == 0, "Unmount MTP before testing"
        claimed = True
        info, _ = command(0x1001)
        assert struct.unpack_from("<I", info, 2)[0] == 6
        command(0x1003)
        command(0x1002, 1)
        opened = True
        stores, _ = command(0x1004)
        assert struct.unpack("<II", stores) == (1, 0x00010001)
        storage, _ = command(0x1005, 0x00010001)
        assert len(storage) >= 26
        handles, _ = command(0x1007, 0x00010001, 0, 0xFFFFFFFF)
        count = struct.unpack_from("<I", handles)[0]
        assert len(handles) == 4 + count * 4
        _, count_response = command(0x1006, 0x00010001, 0, 0xFFFFFFFF)
        assert struct.unpack("<I", count_response)[0] == count
        print(f"MTP discovery, session, storage, listing: PASS ({count} root objects)")
        names = set()
        for offset in range(4, len(handles), 4):
            handle = struct.unpack_from("<I", handles, offset)[0]
            info, _ = command(0x1008, handle)
            assert len(info) >= 53
            units = info[52]
            name = info[53:53 + max(0, units - 1) * 2].decode("utf-16le")
            names.add(name.upper())
            print(f"  {handle}: {name}")
        if options.write_check:
            name = "MT" + secrets.token_hex(3).upper() + ".TST"
            assert name not in names, "Test name already exists"
            for split in [False, True]:
                for size in [0, 1, 51, 52, 53, 64, 500, 512, 1024, 4096, 65536]:
                    data = bytes(index % 251 for index in range(size))
                    _, response = command(0x100C, 0x00010001, 0, data=object_info(name, size), split=split)
                    store, parent, handle = struct.unpack("<III", response)
                    assert store == 0x00010001 and parent == 0 and handle != 0
                    command(0x100D, data=data, split=split)
                    try:
                        downloaded, _ = command(0x1009, handle)
                        assert downloaded == data, f"Readback mismatch: {size}, split={split}"
                        info, _ = command(0x1008, handle)
                        assert struct.unpack_from("<I", info, 8)[0] == size
                        command(0x100C, 0x00010001, 0, data=object_info(name.lower(), 1), expected=0x201D)
                        downloaded, _ = command(0x1009, handle)
                        assert downloaded == data, "Duplicate upload changed existing contents"
                    finally:
                        info, _ = command(0x1008, handle)
                        actual_name = info[53:53 + (info[52] - 1) * 2].decode("utf-16le")
                        assert actual_name.upper() == name, "Refusing to remove another file"
                        command(0x100B, handle)
                    print(f"  upload/readback/delete PASS: {size} bytes, split={split}")
            for invalid in ["TOO-LONG-NAME.TXT", "BAD.", "A..B"]:
                command(0x100C, 0x00010001, 0, data=object_info(invalid, 0), expected=0x2015)
            command(0x100C, 0x00010001, 123, data=object_info(name, 0), expected=0x201A)
            command(0x100C, 0x00010001, 0, data=object_info(name, 100))
            command(0x100D, data=b"short", expected=0x2007)
            after, _ = command(0x1007, 0x00010001, 0, 0xFFFFFFFF)
            assert after == handles, "Failed upload left an object behind"
            print("Upload rejection, rollback and collision checks: PASS")
    finally:
        try:
            if opened:
                command(0x1003)
        finally:
            if claimed:
                usb.libusb_release_interface(device, 0)
            usb.libusb_close(device)
            usb.libusb_exit(None)


if __name__ == "__main__":
    main()
