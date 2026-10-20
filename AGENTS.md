# Project instructions

Before picking up roadmap work from `PLAN.md`, check for `CODE_HEALTH.md`. If present,
its open items come first. Tick items off as they land and remove the backlog when complete.

## Types and data modelling

Use algebraic data types to make illegal states unrepresentable, wherever practical.
Prefer a compile error over a runtime check, and a runtime check over a comment.

- Model mutually exclusive states as one enum with data, not several bools or an
  `Option` plus a flag (see `Life`, `Stance`, `UnitKind`, `GarmentState`, `RigState`).
  Put data that only exists in one state inside that variant (`Life::Downed { time }`,
  `Activation { unit, moved }`).
- Give identifiers and units their own types instead of bare integers (`PawnId`,
  `ItemId`). Use `Option<Id>` rather than a `0` sentinel; `NonZeroU32` keeps it small.
- Match exhaustively on project enums. Avoid `_ =>` arms that would silently absorb
  new variants.
- Never branch on particular content ids or names (`"scrap_metal"`, `"calendar"`).
  Add a typed field or flag to the definition instead, validated when defs load.
  Enums in `defs.ron` are written as strings; deserialize them with
  `#[serde(try_from = "String")]` so typos fail at load with a clear message.
- Strings are fine for open, data-defined sets that are validated at load (palette
  colours, sound names) and for names read from asset files (glTF nodes).
- When a refactor surfaces a combination the old code allowed, check whether it was
  intended before choosing the new type; ask if it changes game rules.

## Clothing, worn armor and character appearance

Before changing apparel geometry, apparel generators or garment rendering, read
[the apparel authoring guide](tools/APPAREL_GUIDE.md). It gives the starting files,
fit recipe, known failure modes and review commands.

- Build on the existing clothing skinning pipeline and the matching male/female
  Peasant outfit skeletons. An inventory prop or a good standing pose is not proof
  that a worn garment fits.
- Review both body types from the front, side and back while aiming and kneeling
  before adding decorative detail. Use the guide's full pose review before finishing.
- Powered armor must enclose the body with a snug, continuous metal shell. The user
  explicitly prefers armor bending with the pawn over rigid plates that float away.
- Leg apparel must have its own worn model, including boots, and render alongside
  upper garments. A full powered suit takes precedence over separate leg equipment.
- Keep generators and generated `.gltf`/`.bin` pairs together. Report CPU preview
  results separately from actual in-game GPU verification.

These instructions apply to apparel work; unrelated tasks do not require an asset review.
