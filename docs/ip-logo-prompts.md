# SmsSink IP 形象 · 出图提示词（Nano Banana Pro）

用法：把下面「通用说明 + 主提示词」整段贴给模型，一次只跑一个方向。
同一段提示词连跑多次会有自然差异，不必手动改词；想故意拉开差异再看第 4 节。

---

## 1. 通用说明（每次都要带，可放在提示词最前面）

本产品是 Windows 托盘常驻的短信接码器：手机把短信推到电脑，自动提取验证码、弹卡片、一键复制。
品牌色深蓝 `#2E4E9E`，视觉气质：安静、可靠、有点萌，不要科技感、不要企业感。

**三条铁律**（照做，否则图不能用）：

1. 提示词里**不要出现** logo、icon、brand mark、app icon、asset 这类词——模型会立刻加上圆角容器、描边、投影、甚至小字。
2. **色值必须写死**。只写「蓝色背景」会得到灰蓝、渐变、暗角。
3. 必须明确 **flat / no outline / no gradient / no shadow / no text**。这四个词缺一个，出来的东西就没法当图标。

---

## 2. A 方向 · 信鸽（推荐先跑这个）

> 语义：手机 → 电脑点对点直飞，不经过任何中转。

```
Create one complete full-bleed 1:1 square image.

Background: fill the entire square with solid muted deep blue #2E4E9E. Keep this blue visible in every open area and in all corners not occupied by the character; the lower-left corner must be occupied by the character. The blue must be one flat uniform color with no gradient, no vignette, and no lighting variation.

Subject: place one extremely simplified, cute, endearing plump pigeon character on the background. View: strict side profile facing left, body upright, chest puffed forward. Defining feature: one short, thick, bluntly rounded beak — with a small soft white bump (the cere) sitting on top of the beak base. Short beak is essential: it must be clearly shorter than the head is tall, never long, flat, or wide like a duck's or a spoonbill's.
Second feature: one folded wing, drawn as a single blunt rounded leaf shape lying tightly against the back and merging with the body contour. Short blunt tail pointing down-back, thick and rounded at the end, never a thin blade or fin.

Complexity: build the whole character from only 4 to 7 large basic shapes, with at most two broad internal color regions. Use one simple round eye and no other facial marks. Remove every nonessential line, outline, feather texture, scale, and decoration. The character must stay readable at 32 x 32.

Color: use exactly three semantic colors in the complete image — cream #FFF6EA for the head and body, warm amber #F0A03C for the folded wing and the beak, and the background blue #2E4E9E. No fourth color. Reuse the amber for the eye.

Composition: the character stands upright and emerges from the lower-left corner, filling about 85 to 95 percent of the square so it stays visually dominant. Cropping at the bottom and at the left side is welcome.

Style: flat vector graphic, ultra-clean, thick soft rounded forms, compact baby-like proportions, large head, calm friendly expression. Purely graphic — not clay, not plastic, not plush, not toy-like, not 3D, not photorealistic.

Constraints: no text, no watermark, no letters, no numbers. No border, no frame, no card, no rounded-square container, no presentation mask. Only one character, no extra subjects, no scenery, no ground, no shadow under the character. No outlines, no strokes, no drop shadows, no gradients, no highlights, no textures, no vignette. Keep flat square outer corners.
```

**出来以后如果……**

| 症状 | 追加一句修正 |
|---|---|
| 又变成鸭子 | `The beak is still too long and too flat. Make it about 40% shorter and much thicker, and put a small round white bump on top of it.` |
| 翅膀像贴上去的色块 | `The wing is floating on top of the body. Push it so it merges into the back contour, so only its front edge is visible.` |
| 尾巴像鱼鳍 | `The tail is too thin and too pointed. Make it shorter, thicker and bluntly rounded.` |
| 背景出现渐变或暗角 | `Flatten the background to one single uniform blue #2E4E9E everywhere, with no gradient and no vignette.` |
| 加了描边 | `Remove all outlines and strokes. Shapes must meet edge to edge with flat color only.` |

---

## 3. C 方向 · 企鹅

> 语义：蓝白品牌色 + 丝滑一步到位、安静常驻。

```
Create one complete full-bleed 1:1 square image.

Background: fill the entire square with solid muted deep blue #2E4E9E. Keep this blue visible in every open area and in both top corners; the lower-left corner must be occupied by the character. One flat uniform color, no gradient, no vignette.

Subject: place one extremely simplified, cute, endearing chubby baby penguin on the background. View: facing the viewer, upright, no neck. Defining feature: one smooth dark hood covering only the top of the head and flowing down the back and sides, so the dark reads as a broad mass, never as a thin rim or outline. The cream face runs continuous into the cream belly as one single shape. Two short blunt flippers merge into the body silhouette at the sides.
Face: two widely spaced simple round eyes, and one small blunt beak placed clearly BELOW the eyes with a clear cream gap between the beak and the eyes.

Complexity: build the whole character from only 4 to 7 large basic shapes. No other facial marks, no eyebrows, no cheeks, no feather texture, no shading. Must stay readable at 32 x 32.

Color: use exactly three semantic colors in the complete image — cream #FFF7EF for the face and belly, dark navy #1B3A5C for the hood, back, flippers, eyes and beak, and the background blue #2E4E9E. No fourth color.

Composition: the character stands upright and emerges from the lower-left corner, filling about 85 to 95 percent of the square. Cropping at the bottom is welcome. Body slightly wider at the bottom than at the head.

Style: flat vector graphic, ultra-clean, thick soft rounded forms, baby-like proportions, calm friendly expression. Purely graphic — not clay, not plastic, not plush, not 3D, not photorealistic.

Constraints: no text, no watermark, no letters, no numbers. No border, no frame, no card, no rounded-square container, no presentation mask. Only one character, no extra subjects, no scenery, no ground, no shadow under the character. No outlines, no strokes, no drop shadows, no gradients, no highlight, no textures, no vignette. Keep flat square outer corners.
```

**出来以后如果……**

| 症状 | 追加一句修正 |
|---|---|
| 像幽灵 / 像裹着斗篷 | `The dark area is too thin and reads as a hooded figure. Make the dark hood on top of the head much thicker and the cream face lower and smaller.` |
| 眼睛和喙糊成一团 | `Move the beak further down and make it smaller, so there is a clear cream gap between the beak and the two eyes.` |
| 白色部分像描边围了一圈 | `The dark must be a thick broad mass on the back and sides, not a thin rim. Widen the dark areas significantly.` |

---

## 4. 想批量拉开差异时，只改这几个词

每次只改一处，其余整段保持不动，六张才有可比性：

- 姿态：`strict side profile facing left` ↔ `three-quarter view turned slightly left`
- 表情：`calm friendly expression` ↔ `sleepy half-closed eyes, tiny gentle smile`
- 比例：`large head` ↔ `very large head, tiny body, oversized cheeks`
- 第二色：`warm amber #F0A03C` → `soft coral #EF7C5A` / `mustard #E3B23C`
- 出画方式：`cropping at the bottom` → `cropping at the bottom and left, shoulder touching the left edge`

---

## 5. 验收清单（决定这张能不能用）

- [ ] 四角是否都是纯色？有没有渐变、暗角、地面投影
- [ ] 是否只有三个颜色？数一下有没有多出第四种
- [ ] 有没有描边、圆角容器、文字、水印
- [ ] 缩到 32px 还能否看出「一只鸟 + 一块第二色」
- [ ] 背景蓝是否接近 `#2E4E9E`（截图取色对比，允许轻微偏差）

四条全过，才值得拿去生 `.ico`。注意：出图后**横向构图必须一致**，否则六个候选没法比较。
