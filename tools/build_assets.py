"""
Build Three Kingdoms themed game assets:
- process classic portraits into avatars/full images
- generate ink-style scene backgrounds
- generate stylized enemy portraits consistent with SG theme
"""
from __future__ import annotations

import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageEnhance, ImageFilter, ImageFont, ImageOps


ROOT = Path('.')
AVATAR_DIR = ROOT / 'sgcard-web' / 'public' / 'avatars'
ENEMY_DIR = ROOT / 'sgcard-web' / 'public' / 'enemies'
BG_DIR = ROOT / 'sgcard-web' / 'public' / 'bg'
CARD_DIR = ROOT / 'sgcard-web' / 'public' / 'cards'
EFFECT_DIR = ROOT / 'sgcard-web' / 'public' / 'effects'
UI_DIR = ROOT / 'sgcard-web' / 'public' / 'ui'

for d in (AVATAR_DIR, ENEMY_DIR, BG_DIR, CARD_DIR, EFFECT_DIR, UI_DIR):
    d.mkdir(parents=True, exist_ok=True)


def load_font(size: int):
    candidates = [
        r'C:\Windows\Fonts\msyh.ttc',
        r'C:\Windows\Fonts\simhei.ttf',
        r'C:\Windows\Fonts\simkai.ttf',
        r'C:\Windows\Fonts\simsun.ttc',
        r'C:\Windows\Fonts\msyhbd.ttc',
    ]
    for path in candidates:
        try:
            return ImageFont.truetype(path, size)
        except Exception:
            continue
    return ImageFont.load_default()


def make_avatar(img: Image.Image, size: int = 512) -> Image.Image:
    img = img.convert('RGBA')
    w, h = img.size
    # Prefer upper body for classic scrolls
    crop_h = int(h * 0.62)
    crop_w = int(min(w, crop_h * 0.92))
    left = max(0, (w - crop_w) // 2)
    top = max(0, int(h * 0.04))
    cropped = img.crop((left, top, left + crop_w, min(h, top + crop_h)))
    # pad to square
    side = max(cropped.size)
    canvas = Image.new('RGBA', (side, side), (245, 240, 232, 255))
    ox = (side - cropped.size[0]) // 2
    oy = (side - cropped.size[1]) // 2
    canvas.paste(cropped, (ox, oy))
    return canvas.resize((size, size), Image.Resampling.LANCZOS)


def make_full(img: Image.Image, width: int = 480) -> Image.Image:
    img = img.convert('RGBA')
    w, h = img.size
    nh = int(h * (width / w))
    return img.resize((width, nh), Image.Resampling.LANCZOS)


def process_source(src: Path, avatar_name: str | None = None, full_name: str | None = None, enemy_name: str | None = None):
    if not src.exists():
        print('missing', src)
        return
    img = Image.open(src)
    if avatar_name:
        av = make_avatar(img, 512)
        out = AVATAR_DIR / avatar_name
        av.save(out, 'PNG', optimize=True)
        print('avatar', out, out.stat().st_size)
    if full_name:
        full = make_full(img, 480)
        out = AVATAR_DIR / full_name
        full.save(out, 'PNG', optimize=True)
        print('full', out, out.stat().st_size)
    if enemy_name:
        enemy = make_avatar(img, 420)
        out = ENEMY_DIR / enemy_name
        enemy.save(out, 'PNG', optimize=True)
        print('enemy', out, out.stat().st_size)


def gradient_bg(size, top, bottom):
    w, h = size
    img = Image.new('RGB', size, top)
    draw = ImageDraw.Draw(img)
    for y in range(h):
        t = y / max(1, h - 1)
        c = tuple(int(top[i] * (1 - t) + bottom[i] * t) for i in range(3))
        draw.line([(0, y), (w, y)], fill=c)
    return img


def draw_mountains(draw: ImageDraw.ImageDraw, w: int, h: int, seed: int, color, y_base: float, amp: float, layers: int = 3):
    rng = random.Random(seed)
    for layer in range(layers):
        pts = []
        y0 = int(h * (y_base + layer * 0.08))
        steps = 24
        for i in range(steps + 1):
            x = int(w * i / steps)
            noise = math.sin(i * 0.7 + layer) * amp + rng.uniform(-amp * 0.3, amp * 0.3)
            y = int(y0 - abs(noise) - layer * 20)
            pts.append((x, y))
        poly = [(0, h)] + pts + [(w, h)]
        alpha = 180 - layer * 35
        fill = (*color, max(40, alpha))
        # pillow polygon needs RGB on RGB image; we draw on RGBA later
        draw.polygon(poly, fill=color if isinstance(color[0], int) and len(color) == 3 else color)


def create_ink_scene(path: Path, kind: str, seed: int = 1):
    w, h = 1600, 900
    rng = random.Random(seed)

    if kind == 'battle':
        base = gradient_bg((w, h), (42, 36, 32), (120, 78, 48))
        # smoke / dust
        overlay = Image.new('RGBA', (w, h), (0, 0, 0, 0))
        od = ImageDraw.Draw(overlay)
        for _ in range(40):
            x = rng.randint(0, w)
            y = rng.randint(h // 3, h)
            r = rng.randint(40, 160)
            od.ellipse((x - r, y - r // 2, x + r, y + r // 2), fill=(30, 20, 10, rng.randint(20, 70)))
        # banners
        for i in range(8):
            x = 120 + i * 180
            od.rectangle((x, 220, x + 10, 620), fill=(40, 20, 10, 200))
            color = [(180, 40, 40), (30, 70, 150), (40, 120, 60), (120, 80, 20)][i % 4]
            od.polygon([(x + 10, 220), (x + 90, 250), (x + 10, 290)], fill=(*color, 220))
        # ground army silhouettes
        for i in range(30):
            x = rng.randint(50, w - 50)
            y = rng.randint(h - 280, h - 80)
            s = rng.randint(18, 36)
            od.ellipse((x - s, y - s // 2, x + s, y + s), fill=(20, 15, 10, 160))
            od.rectangle((x - 4, y - s - 20, x + 4, y - s // 2), fill=(20, 15, 10, 160))
        img = base.convert('RGBA')
        img = Image.alpha_composite(img, overlay)
        title = '古战场'
    elif kind == 'palace':
        base = gradient_bg((w, h), (28, 24, 40), (90, 50, 40))
        overlay = Image.new('RGBA', (w, h), (0, 0, 0, 0))
        od = ImageDraw.Draw(overlay)
        # hall silhouette
        od.rectangle((300, 360, 1300, 820), fill=(35, 20, 15, 220))
        od.polygon([(250, 360), (800, 160), (1350, 360)], fill=(90, 30, 25, 230))
        # pillars
        for x in range(380, 1250, 140):
            od.rectangle((x, 380, x + 36, 820), fill=(70, 35, 25, 230))
        # lanterns
        for x in (420, 700, 980, 1180):
            od.ellipse((x, 430, x + 40, 490), fill=(220, 140, 40, 220))
        img = Image.alpha_composite(base.convert('RGBA'), overlay)
        title = '王城殿阙'
    elif kind == 'map':
        base = gradient_bg((w, h), (236, 226, 200), (190, 176, 140))
        overlay = Image.new('RGBA', (w, h), (0, 0, 0, 0))
        od = ImageDraw.Draw(overlay)
        # rivers
        for i in range(3):
            pts = []
            y = 200 + i * 180
            for x in range(0, w, 40):
                pts.append((x, int(y + math.sin(x / 90 + i) * 40)))
            od.line(pts, fill=(80, 120, 150, 120), width=18)
        # mountains
        for i in range(12):
            x = rng.randint(80, w - 80)
            y = rng.randint(120, h - 160)
            s = rng.randint(40, 110)
            od.polygon([(x, y - s), (x - s, y + s // 2), (x + s, y + s // 2)], fill=(90, 95, 80, 90))
        # city marks
        for i, name in enumerate(['许昌', '成都', '建业', '襄阳', '洛阳']):
            x = 200 + i * 260
            y = 250 + (i % 2) * 180
            od.ellipse((x - 10, y - 10, x + 10, y + 10), fill=(140, 40, 30, 220))
            od.text((x + 14, y - 12), name, font=load_font(28), fill=(60, 40, 20, 230))
        img = Image.alpha_composite(base.convert('RGBA'), overlay)
        title = '九州图'
    else:  # menu / ink landscape
        base = gradient_bg((w, h), (245, 240, 230), (210, 205, 190))
        overlay = Image.new('RGBA', (w, h), (0, 0, 0, 0))
        od = ImageDraw.Draw(overlay)
        # layered mountains
        layers = [
            ((170, 175, 170), 0.62, 90),
            ((110, 115, 110), 0.70, 120),
            ((50, 52, 55), 0.80, 150),
        ]
        for idx, (color, yb, amp) in enumerate(layers):
            pts = []
            steps = 40
            for i in range(steps + 1):
                x = int(w * i / steps)
                y = int(h * yb - (math.sin(i * 0.45 + idx) * amp + math.cos(i * 0.2) * amp * 0.4))
                pts.append((x, y))
            od.polygon([(0, h)] + pts + [(w, h)], fill=(*color, 160 + idx * 20))
        # mist
        for _ in range(25):
            x = rng.randint(0, w)
            y = rng.randint(int(h * 0.45), int(h * 0.85))
            r = rng.randint(60, 180)
            od.ellipse((x - r, y - r // 3, x + r, y + r // 3), fill=(245, 240, 230, rng.randint(30, 80)))
        # sun/moon
        od.ellipse((1180, 120, 1280, 220), fill=(220, 180, 120, 140))
        img = Image.alpha_composite(base.convert('RGBA'), overlay)
        title = '水墨江山'

    # paper texture noise
    noise = Image.effect_noise((w // 2, h // 2), 18).resize((w, h)).convert('L')
    noise_rgba = ImageOps.colorize(noise, black=(0, 0, 0), white=(255, 255, 255)).convert('RGBA')
    noise_rgba.putalpha(28)
    img = Image.alpha_composite(img, noise_rgba)

    # vignette
    vig = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    vd = ImageDraw.Draw(vig)
    for i in range(80):
        alpha = int(i * 1.4)
        vd.rectangle((i, i, w - i, h - i), outline=(20, 15, 10, alpha))
    img = Image.alpha_composite(img, vig)

    # title seal
    d = ImageDraw.Draw(img)
    font = load_font(42)
    d.text((48, 36), f'三国奇谭 · {title}', font=font, fill=(40, 30, 20, 210))
    d.rectangle((48, 90, 170, 210), outline=(150, 40, 30, 220), width=4)
    d.text((68, 120), '汉', font=load_font(56), fill=(150, 40, 30, 230))

    out = img.convert('RGB')
    out = ImageEnhance.Contrast(out).enhance(1.08)
    out = out.filter(ImageFilter.SMOOTH)
    out.save(path, 'JPEG', quality=88, optimize=True)
    print('bg', path, path.stat().st_size)


def create_enemy_portrait(path: Path, name: str, title: str, colors: tuple, symbol: str, seed: int = 1):
    size = 512
    rng = random.Random(seed)
    primary, secondary, accent = colors
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # circular plate
    d.ellipse((16, 16, size - 16, size - 16), fill=(*primary, 255))
    d.ellipse((36, 36, size - 36, size - 36), fill=(*secondary, 255))

    # armor chest
    d.rounded_rectangle((120, 250, 392, 470), radius=40, fill=(*primary, 255))
    d.rounded_rectangle((150, 280, 362, 430), radius=28, fill=(*accent, 230))

    # shoulders
    d.ellipse((70, 240, 170, 340), fill=(*primary, 255))
    d.ellipse((342, 240, 442, 340), fill=(*primary, 255))

    # head
    d.ellipse((170, 90, 342, 280), fill=(232, 198, 164, 255))
    # helmet / hat
    d.polygon([(150, 160), (256, 40), (362, 160)], fill=(*primary, 255))
    d.rectangle((180, 130, 332, 170), fill=(*accent, 255))

    # eyes / beard suggestion
    d.ellipse((210, 170, 235, 190), fill=(30, 20, 15, 255))
    d.ellipse((277, 170, 302, 190), fill=(30, 20, 15, 255))
    if '张' in name or '黄' in name or '寇' in name:
        d.polygon([(200, 230), (256, 280), (312, 230)], fill=(40, 30, 20, 220))

    # decorative ring
    d.ellipse((16, 16, size - 16, size - 16), outline=(*accent, 255), width=8)
    d.ellipse((28, 28, size - 28, size - 28), outline=(245, 230, 180, 180), width=3)

    # text
    d.text((size // 2, 390), name, font=load_font(48), fill=(250, 245, 230, 255), anchor='mm')
    d.text((size // 2, 445), title, font=load_font(28), fill=(245, 220, 160, 255), anchor='mm')
    d.text((size // 2, 70), symbol, font=load_font(36), fill=(255, 240, 200, 255), anchor='mm')

    # ink speckles
    for _ in range(40):
        x, y = rng.randint(20, size - 20), rng.randint(20, size - 20)
        r = rng.randint(1, 3)
        d.ellipse((x - r, y - r, x + r, y + r), fill=(20, 15, 10, rng.randint(20, 80)))

    img.save(path, 'PNG', optimize=True)
    print('enemy-gen', path, path.stat().st_size)


def create_card_back(path: Path):
    w, h = 320, 460
    img = Image.new('RGBA', (w, h), (35, 28, 24, 255))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((10, 10, w - 10, h - 10), radius=18, outline=(200, 160, 70, 255), width=5)
    d.rounded_rectangle((24, 24, w - 24, h - 24), radius=14, outline=(120, 40, 30, 255), width=3)
    d.ellipse((70, 120, 250, 300), outline=(200, 160, 70, 220), width=4)
    d.text((w // 2, 200), '三', font=load_font(72), fill=(220, 180, 80, 255), anchor='mm')
    d.text((w // 2, 270), '国奇谭', font=load_font(34), fill=(240, 220, 170, 255), anchor='mm')
    img.save(path, 'PNG', optimize=True)
    print('card', path)


def create_ui_panel(path: Path):
    w, h = 640, 200
    img = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, w - 1, h - 1), radius=16, fill=(40, 30, 24, 210), outline=(196, 160, 80, 255), width=3)
    img.save(path, 'PNG', optimize=True)
    print('ui', path)


def main():
    # High quality ink portraits already processed for guan/cao/liu/lubu/zhangfei.
    # Process classic public-domain portraits for remaining heroes/bosses.
    process_source(Path('temp_sg_assets/wiki/zhou_yu_classic.jpg'), 'zhou_yu.png', 'zhou_yu_full.png')
    process_source(Path('temp_sg_assets/wiki/zhuge_liang_classic.jpg'), 'zhuge_liang.png', 'zhuge_liang_full.png')
    process_source(Path('temp_sg_assets/wiki/zhang_jue.jpg'), 'zhang_jiao.png', 'zhang_jiao_full.png', 'zhangjiao.png')
    process_source(Path('temp_sg_assets/wiki/cao_cao_scth.jpg'), enemy_name='cao_cao_classic_enemy.png')

    # Reprocess ink masters to ensure consistent outputs
    process_source(Path('temp_sg_assets/portraits/guan_yu_portrait.png'), 'guan_yu.png', 'guan_yu_full.png')
    process_source(Path('temp_sg_assets/portraits/cao_cao_portrait.png'), 'cao_cao.png', 'cao_cao_full.png', 'cao_cao_enemy.png')
    process_source(Path('temp_sg_assets/portraits/liu_bei_portrait.png'), 'liu_bei.png', 'liu_bei_full.png')
    process_source(Path('temp_sg_assets/portraits/lu_bu_portrait.png'), 'lv_bu.png', 'lv_bu_full.png', 'lv_bu_enemy.png')
    process_source(Path('temp_sg_assets/portraits/zhang_fei_portrait.png'), 'zhang_fei.png', 'zhang_fei_full.png', 'zhang_fei_enemy.png')

    # Scenes
    create_ink_scene(BG_DIR / 'main_menu.jpg', 'menu', 11)
    create_ink_scene(BG_DIR / 'battle.jpg', 'battle', 22)
    create_ink_scene(BG_DIR / 'palace.jpg', 'palace', 33)
    create_ink_scene(BG_DIR / 'map.jpg', 'map', 44)
    create_ink_scene(BG_DIR / 'event.jpg', 'menu', 55)
    create_ink_scene(BG_DIR / 'shop.jpg', 'palace', 66)
    # keep compatibility aliases
    create_ink_scene(BG_DIR / 'ancient_china.png'.replace('.png', '.jpg'), 'menu', 11)
    Image.open(BG_DIR / 'main_menu.jpg').save(BG_DIR / 'ancient_china.png', 'PNG', optimize=True)
    Image.open(BG_DIR / 'main_menu.jpg').save(BG_DIR / 'ancient_china_full.png', 'PNG', optimize=True)

    # Enemy stylized portraits for generic troops
    create_enemy_portrait(ENEMY_DIR / 'huangjin_bing.png', '黄巾兵', '黄巾之乱', ((150, 110, 30), (90, 60, 20), (220, 180, 60)), '巾', 1)
    create_enemy_portrait(ENEMY_DIR / 'huangjin_gongshou.png', '黄巾弓手', '远程贼军', ((160, 120, 35), (95, 65, 22), (230, 190, 70)), '弓', 2)
    create_enemy_portrait(ENEMY_DIR / 'huangjin_toumu.png', '黄巾头目', '贼军头领', ((140, 90, 20), (80, 50, 15), (210, 160, 50)), '魁', 3)
    create_enemy_portrait(ENEMY_DIR / 'taiping_daoshi.png', '太平道士', '符水道士', ((70, 90, 120), (30, 50, 80), (180, 200, 230)), '符', 4)
    create_enemy_portrait(ENEMY_DIR / 'liukou.png', '流寇', '乱世草莽', ((90, 70, 50), (50, 35, 25), (170, 130, 90)), '寇', 5)
    create_enemy_portrait(ENEMY_DIR / 'wei_bing.png', '魏兵', '曹魏士卒', ((40, 70, 130), (20, 40, 90), (180, 200, 230)), '魏', 6)
    create_enemy_portrait(ENEMY_DIR / 'wei_jiang.png', '魏将', '曹魏将领', ((30, 55, 120), (15, 30, 80), (210, 180, 90)), '将', 7)
    create_enemy_portrait(ENEMY_DIR / 'yuanshao.png', '袁绍', '四世三公', ((90, 40, 120), (50, 20, 80), (220, 180, 90)), '袁', 8)
    create_enemy_portrait(ENEMY_DIR / 'caocao_jun.svg'.replace('.svg', '.png'), '曹军', '曹魏精锐', ((35, 60, 120), (20, 35, 85), (200, 170, 80)), '军', 9)

    create_card_back(CARD_DIR / 'card_back.png')
    create_ui_panel(UI_DIR / 'panel.png')

    # collage preview for all main characters
    names = [
        ('guan_yu.png', '关羽'),
        ('cao_cao.png', '曹操'),
        ('zhou_yu.png', '周瑜'),
        ('zhuge_liang.png', '诸葛亮'),
        ('lv_bu.png', '吕布'),
        ('liu_bei.png', '刘备'),
        ('zhang_fei.png', '张飞'),
        ('zhang_jiao.png', '张角'),
    ]
    tile = 256
    cols = 4
    rows = 2
    board = Image.new('RGB', (tile * cols, tile * rows), (245, 240, 230))
    for idx, (fname, label) in enumerate(names):
        p = AVATAR_DIR / fname
        if not p.exists():
            continue
        im = Image.open(p).convert('RGB').resize((tile, tile), Image.Resampling.LANCZOS)
        x = (idx % cols) * tile
        y = (idx // cols) * tile
        board.paste(im, (x, y))
        d = ImageDraw.Draw(board)
        d.rectangle((x, y + tile - 36, x + tile, y + tile), fill=(20, 15, 10))
        d.text((x + 12, y + tile - 30), label, font=load_font(24), fill=(240, 220, 170))
    board.save(AVATAR_DIR / 'all_chars.png', 'PNG', optimize=True)
    print('collage', AVATAR_DIR / 'all_chars.png')
    print('ALL DONE')


if __name__ == '__main__':
    main()
