#!/usr/bin/env python3
"""Independent stdlib oracle for SPEC v1; never imports the JS or Rust implementation.

Run with --check to compare committed fixtures; regeneration is an explicit action.
PNG is a test image container, not compression of transferred file data.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1] / 'protocol' / 'vectors'
BLOCK = 4028
IDENTITY = bytes(range(16))


def frame(kind, index, total, payload, n=128):
    size = n*n//4
    output = bytearray(size)
    struct.pack_into('<4sBBBB16sIIH', output, 0, b'VMT1', 1, kind, {128: 1, 160: 2, 256: 3}[n], 0, IDENTITY, index, total, len(payload))
    output[64:64+len(payload)] = payload
    struct.pack_into('<I', output, size-4, zlib.crc32(output[:size-4]))
    return bytes(output)


def png(encoded, scale=3, n=128):
    palette = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255)]
    symbols = [[4]*(n+32) for _ in range(n+32)]
    for x0, y0 in [(4, 4), (n+21, 4), (4, n+21)]:
        for y in range(7):
            for x in range(7):
                symbols[y0+y][x0+x] = 0 if x in (0, 6) or y in (0, 6) or (2 <= x <= 4 and 2 <= y <= 4) else 4
    for i in range(16, n+16):
        symbols[12][i] = symbols[i][12] = 4 if i % 2 else 0
    for c in range(5):
        for y in range(n+20, n+28):
            for x in range(16+c*16, 24+c*16):
                symbols[y][x] = c
    for i, byte in enumerate(encoded):
        for j, shift in enumerate((6, 4, 2, 0)):
            offset = i*4+j
            symbols[16+offset//n][16+offset%n] = (byte >> shift) & 3
    raw = bytearray()
    for row in symbols:
        scan = b'\x00' + b''.join(bytes(palette[c])*scale for c in row)
        raw.extend(scan*scale)

    def chunk(kind, payload):
        return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind+payload))

    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', (n+32)*scale, (n+32)*scale, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')


def pseudo_random(length):
    # SHA-256 counter stream is specified here, independent of PRNG library versions.
    return b''.join(hashlib.sha256(b'VMT1 vectors'+struct.pack('<I', i)).digest()
                    for i in range((length+31)//32))[:length]


def generate():
    cases = [('empty', 'empty.bin', b''), ('one', 'one.bin', b'\x00'),
             ('all-bytes', 'all-bytes.bin', bytes(range(256))),
             ('before-boundary', 'before.bin', pseudo_random(4027)),
             ('boundary', 'boundary.bin', pseudo_random(4028)),
             ('after-boundary', 'after.bin', pseudo_random(4029)),
             ('zeros', 'zeros.bin', bytes(8056)),
             ('random', 'random.bin', pseudo_random(12345)),
             ('unicode', '实验数据-😀.bin', b'\x00\xff\x80\x00\xfe')]
    files, manifest = {}, []
    cases = [(*c, 128) for c in cases]
    for n in (160, 256):
        block = n*n//4-68
        for label, data in [('empty', b''), ('boundary', pseudo_random(block)),
                            ('partial', pseudo_random(block*2+17))]:
            cases.append((f'profile-{n}-{label}', '矩阵-😀.bin', data, n))
    for case, filename, data, n in cases:
        block = n*n//4-68
        total = (len(data)+block-1)//block
        digest = hashlib.sha256(data).digest()
        name = filename.encode('utf-8')
        meta = struct.pack('<QI32sH', len(data), block, digest, len(name))+name
        frames = [frame(1, 0xffffffff, total, meta, n)] + [frame(2, i, total, data[i*block:(i+1)*block], n) for i in range(total)]
        files[case+'/input.bin'] = data
        paths, images = [], []
        for i, encoded in enumerate(frames):
            path = case+f'/frame-{i:03}.bin'
            image = case+f'/frame-{i:03}.png'
            files[path] = encoded
            files[image] = png(encoded, n=n)
            paths.append(path)
            images.append(image)
        manifest.append(dict(case=case, filename=filename, size=len(data), sha256=digest.hex(),
                             id=IDENTITY.hex(), input=case+'/input.bin', frames=paths, images=images))
        if n != 128:
            manifest[-1]['matrix_size'] = n
    files['cases.json'] = (json.dumps(manifest, ensure_ascii=False, indent=2)+'\n').encode()
    return files


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    files = generate()
    for name, content in files.items():
        path = ROOT/name
        if args.check:
            if not path.exists() or path.read_bytes() != content:
                raise SystemExit(f'vector differs: {path}')
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    print(f'{"checked" if args.check else "wrote"} {len(files)} fixed vector files')
