# Prompt recipes & reference

Copy/paste starting points and tuned guidance for the `image-gen` skill. Adapt — keep the
user's requirements, only add detail that materially helps, and don't invent story
elements, brands, or props they didn't ask for.

## Size & quality cheat sheet

| Want | `-Size` | `-Quality` |
| --- | --- | --- |
| Icon / favicon / quick draft | `1024x1024` | `low` |
| Logo mark | `1024x1024` | `medium` |
| Blog/section illustration | `1536x1024` | `medium` |
| Landing-page hero | `1536x1024` (or `2048x1152`) | `high` |
| OG / social card (≈1.91:1) | `2048x1152` | `high` |
| Photoreal / dense text / infographic | as needed | `high` |
| 4K wallpaper | `3840x2160` / `2160x3840` | `high` |

`gpt-image-2` size rules: max edge ≤ 3840px, both edges multiples of 16, ratio ≤ 3:1,
total pixels 655,360–8,294,400. Square renders fastest. Use `auto` if unsure.

## Use-case slugs

`photorealistic-natural · product-mockup · ui-mockup · infographic-diagram ·
scientific-educational · ads-marketing · productivity-visual · logo-brand ·
illustration-story · stylized-concept · historical-scene` and for edits:
`text-localization · identity-preserve · precise-object-edit · lighting-weather ·
background-extraction · style-transfer · compositing · sketch-to-render`.

## Generate recipes

### Landing-page hero background
```
Use case: stylized-concept
Asset type: landing page hero background
Primary request: <subject/idea>
Style/medium: matte illustration / soft-rendered abstract
Composition/framing: wide, with usable negative space for headline + CTA
Lighting/mood: gentle studio glow
Color palette: <brand colors or restrained neutral>
Constraints: no text; no logos; no watermark
```

### Logo mark
```
Use case: logo-brand
Asset type: logo concept
Primary request: <brand idea or symbol>
Style/medium: vector-style flat logo mark; minimal
Composition/framing: single centered mark; strong silhouette; balanced negative space; generous padding
Color palette: 1–2 colors, high contrast
Text (verbatim): "<name>"   # only if a wordmark is wanted
Constraints: no gradients unless essential; no 3D; no mockup; no watermark
```

### App / UI icon (use `-Transparent`)
```
Use case: stylized-concept
Asset type: app icon
Primary request: <icon subject>
Style/medium: clean flat icon; consistent line weight
Composition/framing: centered; generous padding; clear silhouette
Constraints: no text; no background scene; no watermark
```

### Product mockup
```
Use case: product-mockup
Primary request: premium product photo of <product + finish>
Scene/backdrop: clean studio gradient
Composition/framing: centered, slight three-quarter angle, generous padding
Lighting/mood: softbox lighting, controlled shadows
Materials/textures: <matte/gloss/metal…>
Constraints: no logos or trademarks; no watermark
```

### Infographic / diagram (bump `-Quality high`)
```
Use case: infographic-diagram
Primary request: <topic> infographic
Subject: <step 1 -> step 2 -> step 3 …>
Style/medium: clean vector-like infographic with clear callouts and arrows
Composition/framing: <vertical poster | 16:9 slide>, clear top-to-bottom or left-to-right flow
Text (verbatim): "<each label, exactly>"
Constraints: high contrast; readable labels; no watermark
```

### Photoreal blog header
```
Use case: photorealistic-natural
Asset type: blog header
Primary request: <scene>
Style/medium: photorealistic photo; real texture; natural color
Composition/framing: wide crop with clean room for page copy
Lighting/mood: soft natural light, shallow depth of field
Constraints: no text; no logos; no watermark
```

## Edit recipes (use `-InputImage <path> -Edit`)

Always state invariants ("change only X; keep Y unchanged") and repeat them when iterating.

### Recolor / precise object edit
```
Use case: precise-object-edit
Primary request: change only <element> to <new color/material>
Constraints: keep everything else identical — composition, proportions, background, lighting
```

### Background removal (transparent cutout) — prefer `-Transparent` instead
For simple opaque subjects just run generation/edit with `-Transparent`; the engine does the
chroma-key + local alpha extraction. Only escalate to true native transparency (API-key CLI,
`gpt-image-1.5 --background transparent`) for hard subjects — **ask the user first**:
hair, fur, feathers, smoke, glass, liquids, translucent or reflective materials, soft shadows.

### Lighting / weather
```
Use case: lighting-weather
Primary request: make it <winter evening with gentle snowfall | golden hour | overcast>
Constraints: preserve subject identity, geometry, camera angle, composition; change only light/atmosphere/weather
```

### Style transfer (use a style reference, NOT `-Edit`)
```
# -InputImage <style-ref>   (no -Edit: it's a reference, not a target)
Use case: style-transfer
Primary request: apply the reference image's visual style to <new subject>
Constraints: preserve the reference's palette, texture, and brushwork; no extra elements
```

### Compositing (multiple `-InputImage`)
```
# -InputImage <base> <subject>
Use case: compositing
Primary request: place the subject from Image 2 into Image 1 at <where>
Constraints: match lighting, perspective, and scale; keep the base framing unchanged; no extra elements
```

### Character / identity consistency
```
# -InputImage <anchor>   (style+identity reference)
Use case: identity-preserve
Primary request: same character in a new scene: <scene/action>
Style/medium: same style as the reference
Constraints: do not redesign the character; preserve face, proportions, outfit, palette; no text; no watermark
```

## Variants

To explore options, pass `-Variants 3` (three independent takes of one prompt, saved as
`name-1.png`, `name-2.png`, `name-3.png`). For genuinely *different* assets, make separate
calls with distinct prompts — variants are for the same concept.

## Iteration discipline

Start from a clean base prompt, then make **one** targeted change at a time and re-view.
Re-state critical constraints each round. Prefer feeding the chosen output back through
`-InputImage … -Edit` for surgical fixes over rewriting the whole prompt.
