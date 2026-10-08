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

## Composition and validation

Spacing is attached to complete shaped clusters. Regional punctuation side
bearings, shaped narrow forms, tracking, manual Tsume and explicit character aki
are handled before line fitting. Continuous gaps of equal priority share one
allocation pool; non-floating gaps reach discrete endpoints. Leading-only
compression is included for ragged lines. Custom rows retain their dimensions.

Import rejects malformed integers, non-finite amounts, inverted ranges and invalid
priorities. Native document validation uses the same range policy. Unknown integer
classes remain preservable and receive diagnostics, including in nested notes and
cells. The bounded supported spacing range is -1 to 100 em.

## Compatibility limits

[Adobe's TextPreference API](https://developer.adobe.com/indesign/uxp/omv/t/TextPreference/)
defines CID-based classification. The IDML `UseCidMojikumi` preference now survives
round trips and is accessible through `document.preferences`. The engine currently
uses Unicode classes. Preflight reports the incompatibility when the document
requests CID mode and actually renders Adobe-Japan1 CFF fonts with Mojikumi in use;
it does not flag ordinary Unicode fonts merely because the preference is enabled.

Exact Adobe CID-to-Mojikumi class mapping, complete proprietary preset pair matrices,
and legacy pre-CS2 vertical-scaling compatibility have not been reproduced or
certified. Public CMap resources map character codes to CIDs; they are not a
CID-to-Mojikumi-class oracle. No claim of complete Adobe output parity is made.
