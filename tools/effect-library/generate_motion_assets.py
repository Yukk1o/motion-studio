"""Original assets; deterministic toroidal void-and-cluster blue noise.

No downloaded textures or font files. ASCII's default bitmaps are drawn here;
user fonts use the shared Rust rasterizer. Only Python standard library needed.
"""
import json
import math
import random
import struct
import zlib
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
LIB=ROOT/"crates/aem-effects/motion-library"
OUT=LIB/"assets"
OUT.mkdir(parents=True,exist_ok=True)


def png(name,width,height,pixels):
    def chunk(kind,data):
        return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data))
    rows=b''.join(b'\0'+pixels[y*width*4:(y+1)*width*4] for y in range(height))
    data=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',width,height,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(rows,9))+chunk(b'IEND',b'')
    (OUT/name).write_bytes(data)


def blue_noise():
    n=64;count=n*n;rng=random.Random(271828)
    kernel=[(x,y,math.exp(-(x*x+y*y)/4.5)) for y in range(-5,6) for x in range(-5,6)]
    energy=[0.0]*count
    occupied=set(rng.sample(range(count),count//2))
    def update(index,sign):
        x=index%n;y=index//n
        for dx,dy,value in kernel:
            energy[((y+dy)%n)*n+(x+dx)%n]+=value*sign
    def cluster():
        return max(sorted(occupied),key=energy.__getitem__)
    def void():
        return min((i for i in range(count) if i not in occupied),key=energy.__getitem__)
    for i in sorted(occupied):update(i,1)
    for _ in range(count*2):
        remove=cluster();occupied.remove(remove);update(remove,-1)
        add=void();occupied.add(add);update(add,1)
        if remove==add:break
    prototype=set(occupied);prototype_energy=list(energy);ranks=[0]*count
    for rank in range(count//2-1,-1,-1):
        index=cluster();ranks[index]=rank;occupied.remove(index);update(index,-1)
    occupied=prototype;energy[:]=prototype_energy
    for rank in range(count//2,count):
        index=void();ranks[index]=rank;occupied.add(index);update(index,1)
    values=[round(rank*255/(count-1)) for rank in ranks]
    png('blue-noise.png',n,n,bytes(c for v in values for c in [v,v,v,255]))
    # A small, explicit Fourier check accompanies the source tile. No GPU cost claim.
    def power(x,y):
        real=imag=0.0
        for index,value in enumerate(values):
            a=math.tau*(x*(index%n)+y*(index//n))/n
            real+=(value-127.5)*math.cos(a);imag+=(value-127.5)*math.sin(a)
        return (real*real+imag*imag)/count
    low=[power(x,y) for x,y in [(1,0),(0,1),(1,1),(2,0),(0,2)]]
    high=[power(x,y) for x,y in [(24,0),(0,24),(20,20),(28,8),(8,28)]]
    ratio=(sum(low)/len(low))/(sum(high)/len(high))
    assert ratio<0.15,ratio
    (LIB/'blue-noise-spectrum.json').write_text(json.dumps(dict(size=64,seed=271828,method='toroidal_void_and_cluster',low_power=low,high_power=high,low_high_ratio=ratio),indent=2)+'\n')
    print('Blue-noise low/high frequency power ratio:',ratio)


def ascii_atlas():
    patterns=[['00000']*7,['00000']*6+['00100'],['00000','00100','00000','00000','00100','00000','00000'],
              ['00000','00000','00000','11111','00000','00000','00000'],
              ['00100','00100','00100','11111','00100','00100','00100'],
              ['10001','01010','00100','11111','00100','01010','10001'],
              ['01010','11111','01010','11111','01010','11111','01010'],
              ['11111','10001','10101','10101','10101','10001','11111'],
              ['11111','11011','11111','10101','11111','11011','11111'],['11111']*7]
    patterns.sort(key=lambda rows:sum(row.count('1') for row in rows))
    data=bytearray(160*24*4)
    for index,rows in enumerate(patterns):
        for y in range(24):
            for x in range(16):
                on=3<=x<13 and 1<=y<22 and rows[(y-1)//3][(x-3)//2]=='1'
                start=(y*160+index*16+x)*4;data[start:start+4]=bytes([255,255,255,255 if on else 0])
    png('ascii.png',160,24,data)


blue_noise()
ascii_atlas()
pixels=bytearray()
for y in range(64):
    for x in range(64):
        pixels.extend([round((math.sin(math.tau*x/64)*0.5+0.5)*255),round((math.cos(math.tau*y/64)*0.5+0.5)*255),128,255])
png('displacement.png',64,64,pixels)
