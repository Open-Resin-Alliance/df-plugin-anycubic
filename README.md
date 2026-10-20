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

## Mirroring

Mirror flags come from the matching Anycubic entries in the supplied
`Elegoo_officialPlatform.ini`, which explicitly declares `MirrorX`/`MirrorY`.
P1 Max uses X-only mirroring based on the reported physical print orientation.
P1, Photon X, and Mono SQ have no explicit mirror settings in the supplied
references; their previous flags are retained, not considered verified.

Photon Workshop's `rotate_z` is not itself a mirror flag. The two models with
both an explicit 180-degree value and an INI entry (Mono 4 Ultra and Mono M7
Max) use X-only mirroring, consistent with the reported P1 Max result, but
this correlation does not establish the encoder coordinate transformation.
Absent rotation fields occur with X, Y, and no mirroring; no missing mirror
settings are guessed from `rotate_z` or the undocumented `property` integer.

Mono X 6K and M3 Plus share one preset based on physical teardown confirmation.
The INI entries contradict each other (X-only versus Y-only mirroring), so they
do not establish different hardware or a verified common orientation. The shared
preset retains its previous `mirrorX: false`, `mirrorY: false` pending verified
print orientation; those flags are not considered confirmed by the reference.
Corrected existing presets increment `profileVersion` so saved official profiles
can receive the update.

## Native export memory

AFF and AZF file exports spool encoded layer payloads to a scratch file as they
arrive, retaining only layer indices, byte extents and pixel-count metadata.
Out-of-order parallel results keep their original layer identity. Finalization
copies those extents directly into the destination container instead of keeping
all layer payloads and a second complete archive in memory.

PW0 starts with a small output reservation and grows to the actual encoded size;
the reservation is not a mask size or an output limit. Quantization, run lengths,
anti-aliasing, printer resolution, previews and format metadata are unchanged.

The system temporary directory needs space for the encoded layer spool in
addition to the final output file. Scratch files are removed when their encoder
is dropped, including ordinary failure and cancellation paths. Scratch-write
errors fail finalization rather than producing a truncated layer archive.

The APIs that explicitly return container bytes still allocate the returned byte
vector. Desktop/CLI file output uses the streaming path, including progress-enabled
and raw-mask entrypoints.
