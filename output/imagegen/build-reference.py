"""从运行时布局创建生图结构参考；不读取密钥，不调用图片接口。"""
from pathlib import Path
import json
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
layout = json.loads((ROOT / 'src/assets/gba-office/layout.json').read_text())
im = Image.new('RGB', (512, 512), '#739c78')
d = ImageDraw.Draw(im)

def rect(x, y, w, h, color):
    d.rectangle((x, y, x + w - 1, y + h - 1), fill=color)

def border(x, y, w, h, color='#455967', fill='#e8d3a7'):
    rect(x, y, w, h, color)
    rect(x + 1, y + 1, w - 2, h - 2, fill)

def plant(x, y, w=16, h=16):
    rect(x+5,y+10,8,5,'#895e56'); rect(x+6,y+11,6,2,'#d69c70')
    for dx,dy,ww,hh,c in [(5,0,7,10,'#326956'),(1,4,13,6,'#326956'),(3,3,9,7,'#4d9763'),(5,1,5,7,'#75b877'),(2,4,4,3,'#a5cf81')]:
        rect(x+dx,y+dy,ww,hh,c)

def floor(x,y,w,h,kind):
    colors={'office':('#e8d5a7','#d7bd8f'),'meeting':('#c4d5de','#a8bcca'),'lounge':('#eccebd','#d8b4a2'),'reception':('#efe3c5','#d7d0b4'),'balcony':('#d4bd90','#b79c75')}
    a,b=colors[kind]
    rect(x,y,w,h,a)
    for yy in range(y,y+h,16):
        for xx in range(x,x+w,16):
            rect(xx,yy,16,1,b)
            if kind in ('office','balcony'):
                rect(xx+(8 if ((yy-y)//16)%2 else 0),yy,1,16,b)
                rect(xx+3,yy+7,4,1,'#dfc79b')
            else:
                rect(xx,yy,1,16,b)
                if kind=='meeting':rect(xx+6,yy+6,2,2,'#b6cad5')

def chair(col,row,color='#72a2ae'):
    x,y=col*16+3,row*16+3
    rect(x+2,y+10,2,3,'#435367');rect(x+8,y+10,2,3,'#435367')
    border(x,y,12,10,fill=color);rect(x+2,y+2,8,2,'#b6d5d6')

# 外侧是树篱，室内走廊是温暖的浅色方砖。
for yy in range(0,512,8):
    for xx in range(0,512,8):
        rect(xx+2,yy+2,3,2,'#8bb58a');rect(xx,yy+5,4,1,'#668b6c')
rect(16,16,480,480,'#e7e3c9')
for yy in range(16,496,16):
    for xx in range(16,496,16):
        rect(xx,yy,16,1,'#c9cbb4');rect(xx,yy,1,16,'#c9cbb4')

for zone in layout['regions']:
    x,y,w,h=zone['col']*16,zone['row']*16,zone['cols']*16,zone['rows']*16
    floor(x,y,w,h,zone['id'])
    if zone['id']=='balcony':
        for xx in range(x,x+w,16):
            border(xx,y,16,6,fill='#c7d6b0');border(xx,y+h-6,16,6,fill='#c7d6b0')
        for yy in range(y,y+h,16):
            border(x,yy,6,16,fill='#c7d6b0');border(x+w-6,yy,6,16,fill='#c7d6b0')
    else:
        rect(x,y,w,16,'#47646b');rect(x+2,y+2,w-4,5,'#b1cfb8');rect(x+2,y+7,w-4,7,'#d7e2c5')
        for xx in range(x+16,x+w-16,16):rect(xx,y+9,1,4,'#bdcdb0')
        rect(x,y,6,h,'#47646b');rect(x+2,y+16,3,h-19,'#8eafa1')
        rect(x+w-6,y,6,h,'#47646b');rect(x+w-5,y+16,3,h-19,'#b6cbbb')
        rect(x,y+h-6,w,6,'#47646b');rect(x+2,y+h-5,w-4,3,'#a5c5ad')

for door in layout['doorways']:
    x,y=door['col']*16,door['row']*16
    rect(x,y,16,16,'#e7e3c9');rect(x+1,y+1,14,14,'#e0dac1')

def window(col,row):
    x,y=col*16,row*16+2
    border(x,y,32,12,fill='#7eb7ce');rect(x+2,y+2,28,2,'#c9e6df')
    rect(x+15,y+1,2,10,'#e7efda');rect(x+2,y+6,28,1,'#b0d6d6')
for col,row in [(7,1),(12,1),(24,1),(26,12),(5,18),(13,18)]:window(col,row)

# 地毯与门口欢迎垫。
border(55,374,117,79,'#9eae94','#c4cdb0');border(59,378,109,71,'#879b81','#d3dbc0')
border(119,441,80,32,'#698e92','#90b1ad');rect(125,446,68,2,'#c3d9cc');rect(125,466,68,2,'#c3d9cc')
border(363,245,84,69,'#b88787','#e4b4a3')

for obj in layout['furniture']:
    kind=obj['kind'];x,y,w,h=obj['col']*16,obj['row']*16,obj['cols']*16,obj['rows']*16
    rect(x+2,y+h-2,w,h//5+2,'#6a705a')
    if kind=='plant':plant(x,y);continue
    if kind in ('desk','counter','meetingTable','coffeeTable','bistroTable'):
        border(x,y,w,h,fill='#c8a173');rect(x+2,y+2,w-4,3,'#f0d6a0');rect(x+2,y+h-6,w-4,4,'#a57e62')
        for xx in (x+2,x+w-5):rect(xx,y+h-3,3,3,'#5c5e60')
        if kind=='desk':
            border(x+16,y+3,18,13,fill='#a6ccd0');rect(x+18,y+5,14,7,'#587d9c');rect(x+19,y+6,9,1,'#bfded7');rect(x+19,y+9,5,1,'#82b9b7')
            rect(x+23,y+15,4,2,'#596b77');border(x+15,y+19,20,4,fill='#e4e6d3');rect(x+38,y+19,3,4,'#e4e6d3')
            border(x+4,y+12,5,5,fill='#e5a473');rect(x+3,y+10,7,2,'#a4bd8d')
        elif kind=='counter':
            border(x+8,y+4,19,11,fill='#7bb1bb');rect(x+13,y+15,8,2,'#536775');plant(x+w-19,y+3)
            border(x+w//2-8,y+6,16,8,fill='#eee2bd')
        elif kind=='meetingTable':
            border(x+6,y+8,13,10,fill='#e4e3cc');rect(x+w-10,y+8,4,5,'#88bbbd');rect(x+8,y+h-12,4,5,'#d8d9bb')
        else: border(x+5,y+4,8,7,fill='#e4e3cc')
    elif kind=='sofa':
        border(x,y,w,h,fill='#739fa2');rect(x+3,y+2,w-6,4,'#b3cfbd');rect(x+3,y+7,w-6,h-9,'#91b7ad')
        for xx in range(x+16,x+w-5,16):rect(xx,y+2,1,h-4,'#5e848a')
    elif kind in ('cabinet','bookcase'):
        border(x,y,w,h,fill='#b5c2ad');rect(x+2,y+2,w-4,2,'#e6e1bd')
        for yy in range(y+5,y+h-3,8):
            rect(x+2,yy,w-4,1,'#7e9185')
            if kind=='bookcase':
                for xx in range(x+3,x+w-3,5):rect(xx,yy-4,3,4,['#71949e','#c88f83','#ddba75'][(xx//5)%3])
        if kind=='cabinet':rect(x+w-4,y+5,1,3,'#6c7c77')
    elif kind in ('coffeeMachine','arcade','printer'):
        border(x,y,w,h,fill='#8499a5');rect(x+2,y+2,w-4,3,'#c8d8cd')
        border(x+3,y+6,w-6,max(4,h//2),fill='#3d6279');rect(x+5,y+8,w-10,3,'#8cd1cf')
        if kind=='arcade':rect(x+3,y+h-9,w-6,4,'#c27c74');rect(x+5,y+h-8,2,2,'#e4cb79')
        elif kind=='coffeeMachine':border(x+8,y+h-12,7,7,fill='#eee3c1')

for seat in layout['workSeats']+layout['meetingSeats']:chair(seat['col'],seat['row'])
for seat in layout['restSeats'][4:]:chair(seat['col'],seat['row'],'#c2cdb0')

out=ROOT/'output/imagegen/gba-office-structure.png'
im.resize((1024,1024),Image.Resampling.NEAREST).save(out)
print(out)
