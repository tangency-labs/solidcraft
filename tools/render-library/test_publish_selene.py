"""Bounded PNG and explicit-publication regressions; original tiny fixtures only."""
from __future__ import annotations
from pathlib import Path
import struct
import sys
import unittest
import zlib

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from publish_selene import publish, verify_png


def chunk(kind: bytes, content: bytes) -> bytes:
    return struct.pack('>I', len(content))+kind+content+struct.pack('>I', zlib.crc32(kind+content) & 0xffffffff)


def original_fixture(raw: bytes = b'\x00\x10\x20\x30\xff\x40\x50\x60\xff', *, truncated: bool = False) -> bytes:
    compressed = zlib.compress(raw)
    if truncated:
        compressed = compressed[:-2]
    return (b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR', struct.pack('>IIBBBBB', 2, 1, 8, 6, 0, 0, 0))
            +chunk(b'sRGB', b'\x00')+chunk(b'IDAT', compressed)+chunk(b'IEND', b''))


class PublicationTests(unittest.TestCase):
    def test_original_native_shape_fixture_passes_without_resampling(self):
        data = original_fixture()
        record = verify_png(data, 2, 1)
        self.assertEqual(record['bytes'], len(data))
        self.assertEqual(record['crc_and_bounded_scanline_verification'], 'PASS')
        self.assertEqual(record['color_encoding'], 'sRGB')

    def test_wrong_dimensions_tampering_and_truncation_are_not_final_images(self):
        with self.assertRaises(ValueError):
            verify_png(original_fixture(), 1, 2)
        tampered = bytearray(original_fixture())
        tampered[-1] ^= 1
        for data in (bytes(tampered), original_fixture()[:-1], original_fixture()+b'junk', b'not a PNG'):
            with self.assertRaises(ValueError):
                verify_png(data, 2, 1)

    def test_compressed_expansion_and_incomplete_streams_are_bounded(self):
        for data in (original_fixture(b'\x00'*100000), original_fixture(truncated=True)):
            with self.assertRaises(ValueError):
                verify_png(data, 2, 1)
        for dimensions in ((0, 1), (8193, 1), (8192, 8192), (True, 1)):
            with self.assertRaises(ValueError):
                verify_png(original_fixture(), *dimensions)

    def test_publication_requires_review_before_reading_or_creating_files(self):
        with self.assertRaisesRegex(ValueError, 'Inspect the full image'):
            publish(Path('definitely-not-an-output-directory'), 'unreviewed-image', False)


if __name__ == '__main__':
    unittest.main()
