# Test photos

Free-licensed photos for checking cutout quality. They were downloaded from Wikimedia Commons
on 2026-10-13 (the licence of each was checked there), resized to at most 1600 px and
re-saved as JPEG, otherwise unchanged. `graphic-logo.png` is our own: the KreoKit icon on a
flat background.

| File | Case | Licence | Author | Source |
|---|---|---|---|---|
| `person-curly-hair.jpg` | person with hair | CC0 | Jeremy Bishop (Unsplash) | <https://commons.wikimedia.org/wiki/File:Curly_hair_and_freckles_man_(Unsplash).jpg> |
| `cat-tabby.jpg` | pet, fur and whiskers | CC0 | AdinaVoicu (Pixabay) | <https://commons.wikimedia.org/wiki/File:Tabby_cat_with_blue_eyes-3336579.jpg> |
| `dog-grass.jpg` | pet on a busy background | CC0 | Joselodos | <https://commons.wikimedia.org/wiki/File:Border_collie_sitting_on_the_grass.jpg> |
| `product-mug.jpg` | product on a table | CC0 | freestocks.org | <https://commons.wikimedia.org/wiki/File:Mug_of_Coffee_on_a_Wooden_Table_(26804116318).jpg> |
| `low-contrast-polar-bear.jpg` | low contrast | Public domain (U.S. federal government work) | U.S. Fish and Wildlife Service | <https://commons.wikimedia.org/wiki/File:White_polar_bear_on_snow_ursus_maritimus.jpg> |
| `small-subject-snow-dog.jpg` | small subject in a large scene | CC0 | Patrick Hendry (Unsplash) | <https://commons.wikimedia.org/wiki/File:Monochrome_snow_dog_(Unsplash).jpg> |
| `graphic-logo.png` | logo / flat graphic | MIT OR Apache-2.0 (this project) | KreoKit contributors | `assets/brand/kreokit-icon-1024.png` |

## Masks: provisional, not ground truth

`masks/*.provisional-mask.png` were made by **BiRefNet (full, general, epoch 244; MIT)**
<https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-epoch_244.onnx>,
with the input resized to 1024×1024 and the sigmoid output resized back (bilinear). They
are **provisional and not ground truth**: they contain BiRefNet's own mistakes. They are
used only as a regression check (`cargo test --release photo_regression -- --ignored`
warns when a model's IoU drops more than 0.05 below `baseline-fast.json`; it never fails).
Replace them with hand-made masks when those exist.
