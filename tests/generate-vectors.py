#!/usr/bin/env python3
"""Independent stdlib oracle for VMT1/VMT2; never imports the JS or Rust implementation.

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


def dimensions(n):
    return {"1028x512": (1028, 512), "512x256": (512, 256), "256x128": (256, 128)}.get(n, (n, n))


def capacity(n, mode):
    width, height = dimensions(n)
    return width*height*(3 if mode == 24 else {2: 1, 4: 2, 8: 3, 16: 4}[mode]/8)


def frame(kind, index, total, payload, n=128, mode=4):
    width, height = dimensions(n)
    size = int(capacity(n, mode))
    output = bytearray(size)
    struct.pack_into('<4sBBBB16sIIH', output, 0, b'VMT1', 1 if mode == 4 else 2, kind, {128: 1, 160: 2, 256: 3, 512: 4, "1028x512": 5, "512x256": 6, "256x128": 7}[n], 0 if mode == 4 else mode, IDENTITY, index, total, len(payload) & 0xffff)
    if n == "1028x512" or mode != 4:
        struct.pack_into('<H', output, 34, len(payload) >> 16)
    output[64:64+len(payload)] = payload
    struct.pack_into('<I', output, size-4, zlib.crc32(output[:size-4]))
    return bytes(output)


def png(encoded, scale=3, n=128, mode=4):
    palette = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255)]
    palettes = {2: [(0, 0, 0), (255, 255, 255)],
                8: palette[:4]+[(255, 255, 0), (255, 0, 255), (0, 255, 255), (255, 255, 255)],
                16: palette[:4]+[(255, 255, 0), (255, 0, 255), (0, 255, 255), (255, 255, 255),
                     (128, 0, 0), (0, 128, 0), (0, 0, 128), (128, 128, 0), (128, 0, 128),
                     (0, 128, 128), (128, 128, 128), (255, 128, 0)]}
    width, height = dimensions(n)
    symbols = [[4]*(width+32) for _ in range(height+32)]
    for x0, y0 in [(4, 4), (width+21, 4), (4, height+21)]:
        for y in range(7):
            for x in range(7):
                symbols[y0+y][x0+x] = 0 if x in (0, 6) or y in (0, 6) or (2 <= x <= 4 and 2 <= y <= 4) else 4
    for i in range(16, width+16):
        symbols[12][i] = 4 if i % 2 else 0
    for i in range(16, height+16):
        symbols[i][12] = 4 if i % 2 else 0
    if mode == 4:
        for c in range(5):
            for y in range(height+20, height+28):
                for x in range(16+c*16, 24+c*16): symbols[y][x] = c
    else:
        code = {2: 1, 8: 2, 16: 3, 24: 4}[mode]
        for repeat in range(4):
            for bit in range(3): symbols[13][16+repeat*3+bit] = 0 if code & (1 << (2-bit)) else 4
        swatches = palette if mode == 24 else palettes[mode]+[(0, 0, 0), (255, 255, 255)]
        for i, rgb in enumerate(swatches):
            for y in range(height+18+(i//9)*7, height+24+(i//9)*7):
                for x in range(16+(i%9)*16, 22+(i%9)*16): symbols[y][x] = rgb
    count = width*height
    bits = {2: 1, 4: 2, 8: 3, 16: 4}.get(mode)
    for i in range(count):
        if mode == 24: value = tuple(encoded[i*3:i*3+3])
        else:
            value = 0
            for j in range(bits):
                bit = i*bits+j
                value = (value << 1) | ((encoded[bit//8] >> (7-bit%8)) & 1)
            if mode != 4: value = palettes[mode][value]
        symbols[16+i//width][16+i%width] = value
    raw = bytearray()
    for row in symbols:
        scan = b'\x00' + b''.join(bytes(c if isinstance(c, tuple) else palette[c])*scale for c in row)
        raw.extend(scan*scale)

    def chunk(kind, payload):
        return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind+payload))

    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', (width+32)*scale, (height+32)*scale, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')


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
    cases = [(*c, 128, 4) for c in cases]
    for n in (160, 256, 512, "1028x512", "512x256", "256x128"):
        width, height = dimensions(n)
        block = width*height//4-68
        for label, data in [('empty', b''), ('boundary', pseudo_random(block)),
                            ('partial', pseudo_random(block*2+17))]:
            cases.append((f'profile-{n}-{label}', '矩阵-😀.bin', data, n, 4))
    for mode in (2, 8, 16, 24):
        block = int(capacity(128, mode))-68
        cases.extend([(f'color-{mode}-empty', '颜色-😀.bin', b'', 128, mode),
                      (f'color-{mode}-partial', '颜色-😀.bin', pseudo_random(block+17), 128, mode)])
    for case, filename, data, n, mode in cases:
        width, height = dimensions(n)
        block = int(capacity(n, mode))-68
        total = (len(data)+block-1)//block
        digest = hashlib.sha256(data).digest()
        name = filename.encode('utf-8')
        meta = struct.pack('<QI32sH', len(data), block, digest, len(name))+name
        frames = [frame(1, 0xffffffff, total, meta, n, mode)] + [frame(2, i, total, data[i*block:(i+1)*block], n, mode) for i in range(total)]
        files[case+'/input.bin'] = data
        paths, images = [], []
        for i, encoded in enumerate(frames):
            path = case+f'/frame-{i:03}.bin'
            image = case+f'/frame-{i:03}.png'
            files[path] = encoded
            files[image] = png(encoded, n=n, mode=mode)
            paths.append(path)
            images.append(image)
        manifest.append(dict(case=case, filename=filename, size=len(data), sha256=digest.hex(),
                             id=IDENTITY.hex(), input=case+'/input.bin', frames=paths, images=images))
        if n != 128:
            manifest[-1]['matrix_size'] = n
        if mode != 4:
            manifest[-1]['color_mode'] = mode
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
