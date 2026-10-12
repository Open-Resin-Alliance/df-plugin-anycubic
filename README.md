# df-plugin-anycubic

DragonFruit Plugin for Anycubic Printers

> **Asset source notice**
>
> Printer preview assets in this repository were sourced from the official Anycubic website.
> We are currently seeking explicit permission from Anycubic for their use and distribution.

## Bit depth

All Anycubic printer presets declare 4-bit grayscale (16 levels) for anti-aliasing.

The Photon Mono SE, Photon Mono X, and Photon Mono X 6K / M3 Plus keep the 4-bit
setting but have anti-aliasing disabled (`antiAliasing: false`), as they are
time-slice printers.
