# Mojikumi implementation and public references

This is a clean-room implementation of public spacing behavior. Mojikumi is an
Adobe feature, not a single conformance standard. W3C JLReq and CLReq describe
Japanese and Chinese layout requirements; they do not specify Adobe's private
composer or its complete preset pair matrices.

References:

- [Adobe preset enumeration](https://developer.adobe.com/indesign/uxp/dom/api/m/mojikumi-table-defaults/)
- [Adobe Mojikumi guide, sections 3–5](https://wwwimages.adobe.com/www.adobe.com/jp/joc/design/guides/pdf/mojikumi.pdf)
- [Adobe spacing priorities](https://helpx.adobe.com/indesign/desktop/language-and-proofing/chinese-japanese-and-korean/set-spacing-priorities-in-mojikumi-character-classes.html)
- [W3C JLReq](https://www.w3.org/TR/jlreq/), especially character classes and line adjustment
- [W3C CLReq](https://www.w3.org/TR/clreq/), especially regional punctuation placement

## Presets and interchange

The resolver recognizes the 14 Japanese presets and two Chinese presets from the
public enumeration, including their IDML resource aliases. Paragraph indentation,
opening-bracket indentation, ordinary line starts, full/half/discrete line ends,
and period-only line-end policies are represented separately. Custom IDML rows
replace the corresponding directional pair. Adjacent opening and closing marks
must not regain two full blank half-bodies.

Regression tests cover preset boundary differences, custom override precedence,
IDML reference round trips, and unsupported names. The implementation must not be
represented as a byte-for-byte reproduction of Adobe's full preset matrices.

## Audit work

Further audit areas are regional punctuation metrics, shaped cluster boundaries,
equal-priority allocation, discrete line fitting, input validation, and diagnostics
for compatibility modes whose glyph classification cannot yet be reproduced.
