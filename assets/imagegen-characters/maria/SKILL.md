---
name: maria-character
version: 1.0.0
description: Reusable visual character definition for Maria, a fictional AI-generated woman, for consistent ImageGen use in Sprøyt and ChatGPT.
---

# Maria — reusable ImageGen character skill

## Purpose
Use this skill whenever the user asks for **Maria** in an image. Maria is a fictional, AI-generated recurring character. Preserve her visual identity across scenes while allowing natural changes in expression, pose, clothing, lighting, camera angle and artistic style.

## Canonical source
The strongest identity anchor is `references/00-canonical-maria.jpg`. Use it as the primary face/identity reference whenever reference-image conditioning is available.

Use `references/01-expression-and-context-sheet.png` as a secondary identity sheet. The other images demonstrate successful variations, not separate identities.

## Visual identity
Maria is an adult woman with:
- light/medium warm skin tone, often lightly sun-kissed;
- medium-long, naturally wavy, slightly tousled dark-blonde/light-brown hair with warmer blonde highlights;
- expressive brown/hazel eyes;
- natural eyebrows and understated makeup;
- an oval/softly angular face, defined cheekbones, straight-to-soft nose and natural lips;
- a lean, healthy adult build with long proportions;
- a natural, unposed photographic presence rather than a fashion-model look.

The key is **recognition, not exact duplication of one pose**. Preserve face geometry, hair character, age range and overall identity while allowing believable variation.

## Character / expression direction
Maria should feel like a real recurring person rather than a stock-photo model. She can be cheerful, amused, tired, irritated, skeptical, concentrated, surprised, playful, worried, relaxed or quietly content.

Do **not** default every image to the same pose, especially the recurring “chin resting in hand + small crooked smile” pose. Vary gaze direction, posture, facial muscle tension, hand placement and energy according to the scene.

Her expressions should normally be subtle and credible. Prefer candid micro-expressions over exaggerated commercial smiles unless the scene explicitly calls for exaggeration.

## Photographic style
Unless the user requests another style:
- realistic/candid photography;
- natural skin texture, not plastic beauty retouching;
- believable phone-camera or documentary-camera perspective;
- physically plausible light and depth of field;
- environmental imperfections are welcome;
- avoid excessive glamour styling;
- integrate Maria into the supplied scene rather than making her look pasted in.

When editing a real photo, preserve the original image's camera position, lens feel, lighting, noise, sharpness, color response and depth of field. Maria should inherit the same image quality and focus as surrounding people.

## Clothing
Clothing follows the requested scene. Established successful looks include:
- casual grey sleep shirt;
- dark raincoat, knitted scarf, dark trousers and light sneakers;
- loose white linen shirt/cover-up in Greece;
- cream knitwear;
- casual denim and summer clothes;
- fitted cream cashmere/knit top with loose sleeves and a long patterned skirt with a high slit;
- professional black bar T-shirt/apron when working behind a bar;
- dirndl/Oktoberfest clothing when explicitly requested.

Do not treat any one outfit as canonical. Identity must survive wardrobe changes.

## Greece / Mediterranean visual vocabulary
For Greek scenes, Maria works especially well with whitewashed walls, blue doors/shutters, stone lanes, bougainvillea, warm sun, seaside tavernas, beach bars and Cycladic architecture. These are scene cues, not part of her identity.

## Stylized renderings
Maria may be rendered as oil painting, illustration, label art or other styles. Preserve recognizable facial proportions, hair shape/color and expression cues even when stylized.

For oil-painting requests, `references/09-oil-painting-style.png` is a useful style reference, but do not let painterly stylization change her identity.

## Composition rules
When the user supplies a base image and asks to add Maria:
1. Preserve the base composition unless instructed otherwise.
2. Put Maria exactly where requested.
3. Match perspective, scale, light direction, focal plane and blur.
4. Preserve existing people and objects unless replacement is explicitly requested.
5. If Maria is in the background, she should be no sharper or higher-quality than subjects at the same depth.
6. If seen from behind or in profile, preserve her characteristic wavy highlighted hair and body proportions without forcing her face into view.

## Prompt fragment
When a text identity description is needed, use this as a compact starting point:

> Maria, the established recurring fictional adult woman from the supplied references: warm light/medium skin, medium-long naturally wavy tousled dark-blonde/light-brown hair with warm blonde highlights, expressive hazel-brown eyes, softly angular oval face, defined cheekbones, natural lips, lean healthy build, candid believable presence. Preserve her established facial identity and hair character; adapt expression, pose and wardrobe naturally to the scene.

Do not rely on this text alone when reference images can be passed to ImageGen; image references are substantially more useful for identity consistency.

## Negative / drift prevention
Avoid:
- changing Maria into a generic blonde model;
- strongly changing age, face width, nose, eye spacing or jawline;
- overly smooth/airbrushed skin;
- identical expression in every image;
- permanently posing her with chin in hand;
- making background Maria unnaturally sharper than the source photo;
- inventing text/signage unless requested;
- changing unrelated people in image-edit tasks.

## Reference priority
1. `00-canonical-maria.jpg` — primary identity anchor.
2. `01-expression-and-context-sheet.png` — range/consistency.
3. `03-happy-bus-selfie.png`, `04-greek-beach.png`, `06-kitchen-expression.png` — expression range.
4. `05-cafe-from-behind.png`, `08-greek-street-fashion.png` — rear/profile/body/hair cues.
5. `07-bartender.png` — role/wardrobe variation.
6. `09-oil-painting-style.png` — stylized rendering only.

## Sprøyt integration recommendation
Store a stable `character_id = "maria"` that resolves to this skill plus the canonical reference images. For every ImageGen request containing Maria, attach `00-canonical-maria.jpg`; attach one additional context reference when it closely matches the requested angle/expression/style. Keep scene instructions separate from identity instructions so scene changes do not overwrite character identity.
