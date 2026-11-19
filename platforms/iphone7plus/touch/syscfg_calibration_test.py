import importlib.util
from pathlib import Path
import struct


path = Path(__file__).with_name("syscfg-calibration.py")
spec = importlib.util.spec_from_file_location("syscfg_calibration", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def fixture():
    data = bytearray(24 + 4 * 20 + 32)
    data[:4] = b"gfCS"
    struct.pack_into("<I", data, 8, len(data))
    struct.pack_into("<I", data, 20, 4)
    for index, name in enumerate(module.RECORDS):
        offset = 24 + index * 20
        if index == 0:
            data[offset:offset + 4] = b"BTNC"
            data[offset + 4:offset + 8] = name[::-1].encode()
            struct.pack_into("<II", data, offset + 8, 32, 104)
        else:
            data[offset:offset + 4] = name[::-1].encode()
            data[offset + 4:offset + 20] = bytes([index]) * 16
    data[104:] = b"M" * 32
    return data


data = fixture()
records = module.read_records(data)
assert records["MtCl"] == b"M" * 32
assert records["OrbG"] == b"\x01" * 16
assert records["OFCl"] == b"\x02" * 16
assert records["SDAC"] == b"\x03" * 16

bad = bytearray(data)
bad[:4] = b"junk"
try:
    module.read_records(bad)
    raise AssertionError("bad magic accepted")
except ValueError:
    pass

bad = bytearray(data)
struct.pack_into("<I", bad, 32, 9999)
try:
    module.read_records(bad)
    raise AssertionError("out-of-bounds record accepted")
except ValueError:
    pass

bad = bytearray(data)
bad[24 + 3 * 20:24 + 3 * 20 + 4] = b"junk"
try:
    module.read_records(bad)
    raise AssertionError("missing calibration accepted")
except ValueError:
    pass
