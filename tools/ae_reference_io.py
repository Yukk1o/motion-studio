"""Decode reference frames while preserving the AE output module's Alpha contract."""
import io
import struct
from pathlib import Path
from PIL import Image


def read_ae_tiff(path: Path):
    data = bytearray(path.read_bytes())
    with Image.open(io.BytesIO(data)) as image:
        tags = image.tag_v2
        # AE 18's Photoshop/TIFF writer uses an unspecified fourth sample even
        # though its output module records RGB+Alpha, premultiplied over black.
        # Tell Pillow it is associated Alpha so it decodes all four channels and
        # unpremultiplies RGB. Only patch an in-memory copy; retain raw evidence.
        if tags.get(277) == 4 and tags.get(338) == (0,):
            endian = "<" if data[:2] == b"II" else ">"
            if struct.unpack_from(endian + "H", data, 2)[0] != 42:
                raise ValueError("Unsupported AE TIFF encoding")
            offset = struct.unpack_from(endian + "I", data, 4)[0]
            count = struct.unpack_from(endian + "H", data, offset)[0]
            for i in range(count):
                entry = offset + 2 + 12 * i
                tag, kind, length = struct.unpack_from(endian + "HHI", data, entry)
                if tag == 338 and kind == 3 and length == 1:
                    struct.pack_into(endian + "H", data, entry + 8, 1)
                    break
            else:
                raise ValueError("AE Alpha metadata is missing")
    with Image.open(io.BytesIO(data)) as image:
        if image.mode != "RGBA":
            raise ValueError("AE output does not contain RGB+Alpha")
        image.load()
        return image.copy()
