"""Strings and data addresses a function materialises (lis rX,hi + addi rY,rX,lo in its asm), with the printable
string at each address from the memory image.

usage: PPC_IMAGE=<image.bin> RECOMP_GENERATED=<recomp generated dir> py -3.13 fnstrings.py <function hex, e.g. 826E5E78>

PPC_IMAGE is a dump of the executable's loaded memory image (base 0x82000000) made from your own copy of the game;
RECOMP_GENERATED is your own skate3recomp build's generated/ folder (read by fn.sh next to this script).
"""
import os, re, subprocess, sys
from pathlib import Path
IMG = os.environ['PPC_IMAGE']
FN = str(Path(__file__).resolve().parent / 'fn.sh')
data = open(IMG, 'rb').read()
asm = subprocess.run(['bash', FN, sys.argv[1].upper()], capture_output=True, text=True).stdout
hi = {}
for line in asm.splitlines():
    m = re.match(r'\s*lis (r\d+),(-?\d+)', line)
    if m:
        hi[m.group(1)] = int(m.group(2)) & 0xFFFF
        continue
    m = re.match(r'\s*(?:addi|lwz|lbz|lfs|stw|stb) (r\d+),(?:(r\d+),(-?\d+)|(-?\d+)\((r\d+)\))', line)
    if m:
        src, lo = (m.group(2), m.group(3)) if m.group(2) else (m.group(5), m.group(4))
        if src in hi:
            a = ((hi[src] << 16) + int(lo)) & 0xFFFFFFFF
            off = a - 0x82000000
            s = ''
            if 0 <= off < len(data):
                mm = re.match(rb'[\x20-\x7e]{4,}', data[off:off + 80])
                s = mm.group().decode() if mm else ''
            print(f'{a:08X} {line.strip():40s} {s}')
