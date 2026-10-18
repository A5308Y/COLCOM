//! RimWorld-style health: a body made of parts with hit points, health
//! conditions ("hediffs") attached to parts or the whole body, and the
//! capacities (consciousness, moving, ...) that follow from them.

use crate::rng::Rng;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

/// Fraction of total blood the body regenerates per day.
const BLOOD_REGEN: f32 = 0.33;
/// Pain at or above this knocks a pawn down.
pub const PAIN_SHOCK: f32 = 0.8;
/// How long a disease stays tended before it needs tending again.
const DISEASE_TEND_TIME: f32 = 0.5 * DAY_LEN;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Part {
    Torso,
    Neck,
    Head,
    Skull,
    Brain,
    LeftEye,
    RightEye,
    LeftEar,
    RightEar,
    Nose,
    Jaw,
    Spine,
    Heart,
    LeftLung,
    RightLung,
    LeftKidney,
    RightKidney,
    Liver,
    Stomach,
    LeftShoulder,
    LeftArm,
    LeftHand,
    RightShoulder,
    RightArm,
    RightHand,
    LeftLeg,
    LeftFoot,
    RightLeg,
    RightFoot,
}

pub struct PartDef {
    pub name: &'static str,
    pub parent: Option<Part>,
    pub max_hp: f32,
    /// Relative chance of being hit (outer parts), or of a deep blow reaching it (inner parts).
    pub hit_weight: f32,
    /// Sits inside its parent and is only reached by blows that go deep.
    pub inside: bool,
    /// Losing it is fatal.
    pub vital: bool,
    /// Bleeding multiplier for wounds here.
    pub bleed: f32,
}

#[allow(clippy::too_many_arguments)]
fn d(name: &'static str, parent: Option<Part>, max_hp: f32, hit_weight: f32, inside: bool, vital: bool, bleed: f32) -> PartDef {
    PartDef { name, parent, max_hp, hit_weight, inside, vital, bleed }
}

impl Part {
    pub const ALL: [Part; 29] = [
        Part::Torso,
        Part::Neck,
        Part::Head,
        Part::Skull,
        Part::Brain,
        Part::LeftEye,
        Part::RightEye,
        Part::LeftEar,
        Part::RightEar,
        Part::Nose,
        Part::Jaw,
        Part::Spine,
        Part::Heart,
        Part::LeftLung,
        Part::RightLung,
        Part::LeftKidney,
        Part::RightKidney,
        Part::Liver,
        Part::Stomach,
        Part::LeftShoulder,
        Part::LeftArm,
        Part::LeftHand,
        Part::RightShoulder,
        Part::RightArm,
        Part::RightHand,
        Part::LeftLeg,
        Part::LeftFoot,
        Part::RightLeg,
        Part::RightFoot,
    ];

    pub fn def(self) -> PartDef {
        use Part::*;
        match self {
            Torso => d("Torso", None, 40.0, 30.0, false, true, 1.0),
            Neck => d("Neck", Some(Torso), 25.0, 4.0, false, true, 2.0),
            Head => d("Head", Some(Neck), 25.0, 6.0, false, true, 1.0),
            Skull => d("Skull", Some(Head), 25.0, 1.0, true, false, 0.5),
            Brain => d("Brain", Some(Skull), 10.0, 1.0, true, true, 1.5),
            LeftEye => d("Left eye", Some(Head), 10.0, 0.8, false, false, 0.5),
            RightEye => d("Right eye", Some(Head), 10.0, 0.8, false, false, 0.5),
            LeftEar => d("Left ear", Some(Head), 12.0, 0.8, false, false, 0.5),
            RightEar => d("Right ear", Some(Head), 12.0, 0.8, false, false, 0.5),
            Nose => d("Nose", Some(Head), 10.0, 0.7, false, false, 0.5),
            Jaw => d("Jaw", Some(Head), 20.0, 1.2, false, false, 0.8),
            Spine => d("Spine", Some(Torso), 25.0, 1.5, true, false, 0.8),
            Heart => d("Heart", Some(Torso), 15.0, 1.5, true, true, 3.0),
            LeftLung => d("Left lung", Some(Torso), 15.0, 2.0, true, false, 2.0),
            RightLung => d("Right lung", Some(Torso), 15.0, 2.0, true, false, 2.0),
            LeftKidney => d("Left kidney", Some(Torso), 15.0, 1.2, true, false, 1.5),
            RightKidney => d("Right kidney", Some(Torso), 15.0, 1.2, true, false, 1.5),
            Liver => d("Liver", Some(Torso), 20.0, 2.2, true, true, 2.0),
            Stomach => d("Stomach", Some(Torso), 20.0, 2.0, true, false, 1.5),
            LeftShoulder => d("Left shoulder", Some(Torso), 30.0, 4.0, false, false, 1.0),
            LeftArm => d("Left arm", Some(LeftShoulder), 30.0, 6.0, false, false, 1.0),
            LeftHand => d("Left hand", Some(LeftArm), 20.0, 2.5, false, false, 0.6),
            RightShoulder => d("Right shoulder", Some(Torso), 30.0, 4.0, false, false, 1.0),
            RightArm => d("Right arm", Some(RightShoulder), 30.0, 6.0, false, false, 1.0),
            RightHand => d("Right hand", Some(RightArm), 20.0, 2.5, false, false, 0.6),
            LeftLeg => d("Left leg", Some(Torso), 30.0, 9.0, false, false, 1.0),
            LeftFoot => d("Left foot", Some(LeftLeg), 25.0, 3.0, false, false, 0.6),
            RightLeg => d("Right leg", Some(Torso), 30.0, 9.0, false, false, 1.0),
            RightFoot => d("Right foot", Some(RightLeg), 25.0, 3.0, false, false, 0.6),
        }
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }

    /// True if this part is `ancestor` or sits (indirectly) inside/on it.
    pub fn is_within(self, ancestor: Part) -> bool {
        let mut cur = Some(self);
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = p.def().parent;
        }
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DamageKind {
    Cut,
    Gunshot,
    Bruise,
    /// From a laser: seared shut (no bleeding), but it hurts more.
    Burn,
}

impl DamageKind {
    pub fn name(self) -> &'static str {
        match self {
            DamageKind::Cut => "Cut",
            DamageKind::Gunshot => "Gunshot",
            DamageKind::Bruise => "Bruise",
            DamageKind::Burn => "Burn",
        }
    }

    /// Blood lost per day per point of wound severity.
    fn bleed(self) -> f32 {
        match self {
            DamageKind::Cut | DamageKind::Gunshot => 0.06,
            DamageKind::Bruise | DamageKind::Burn => 0.0,
        }
    }

    /// Pain per point of wound severity.
    fn pain(self) -> f32 {
        match self {
            DamageKind::Burn => 0.022,
            _ => 0.015,
        }
    }

    /// Chance per layer of going deeper into the body.
    fn penetration(self) -> f32 {
        match self {
            DamageKind::Gunshot => 0.6,
            DamageKind::Cut => 0.35,
            DamageKind::Bruise => 0.2,
            DamageKind::Burn => 0.45,
        }
    }

    fn scar_chance(self) -> f32 {
        match self {
            DamageKind::Gunshot => 0.12,
            DamageKind::Cut => 0.08,
            DamageKind::Bruise => 0.0,
            DamageKind::Burn => 0.15,
        }
    }

    fn can_infect(self) -> bool {
        self != DamageKind::Bruise
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum HediffKind {
    /// Damage to a part. Permanent injuries are scars.
    Injury { kind: DamageKind, permanent: bool },
    /// The part (and everything on it) is gone.
    Missing,
    Infection { immunity: f32 },
    Flu { immunity: f32 },
    /// From bad food: miserable for a while, then gone. Severity 1 → 0.
    FoodPoisoning,
    /// A fitted replacement for a lost part (and everything on it).
    Prosthetic { efficiency: f32 },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Hediff {
    pub kind: HediffKind,
    /// None = whole body.
    pub part: Option<Part>,
    /// Injuries: hit points of damage. Diseases: 0..1, fatal at 1.
    pub severity: f32,
    pub peak: f32,
    /// Quality (0..1) of the current treatment.
    pub tend: Option<f32>,
    pub tend_left: f32,
    /// Seconds until the wound either gets infected or is safe (< 0 = no check).
    pub infection_timer: f32,
    pub age: f32,
}

impl Hediff {
    fn new(kind: HediffKind, part: Option<Part>, severity: f32) -> Hediff {
        Hediff { kind, part, severity, peak: severity, tend: None, tend_left: 0.0, infection_timer: -1.0, age: 0.0 }
    }

    pub fn is_disease(&self) -> bool {
        matches!(self.kind, HediffKind::Infection { .. } | HediffKind::Flu { .. })
    }

    pub fn immunity(&self) -> Option<f32> {
        match self.kind {
            HediffKind::Infection { immunity } | HediffKind::Flu { immunity } => Some(immunity),
            _ => None,
        }
    }

    /// Fraction of total blood lost per day.
    pub fn bleed(&self) -> f32 {
        if self.tend.is_some() {
            return 0.0;
        }
        let part_bleed = self.part.map_or(1.0, |p| p.def().bleed);
        match self.kind {
            HediffKind::Injury { kind, permanent: false } => self.severity * kind.bleed() * part_bleed,
            HediffKind::Missing => self.part.map_or(0.0, |p| p.def().max_hp) * 0.05 * part_bleed,
            _ => 0.0,
        }
    }

    pub fn pain(&self) -> f32 {
        match self.kind {
            HediffKind::Injury { kind, permanent: false } => self.severity * kind.pain(),
            HediffKind::Injury { permanent: true, .. } => self.severity * 0.008,
            HediffKind::Missing => {
                if self.age < 2.0 * DAY_LEN {
                    0.12
                } else {
                    0.0
                }
            }
            HediffKind::Infection { .. } => self.severity * 0.3,
            HediffKind::Flu { .. } => 0.05 + self.severity * 0.1,
            HediffKind::FoodPoisoning => 0.2 * self.severity,
            HediffKind::Prosthetic { .. } => 0.0,
        }
    }

    pub fn needs_tending(&self) -> bool {
        match self.kind {
            HediffKind::Injury { permanent: true, .. } | HediffKind::FoodPoisoning | HediffKind::Prosthetic { .. } => false,
            _ => self.tend.is_none(),
        }
    }

    pub fn label(&self) -> String {
        match self.kind {
            HediffKind::Injury { kind, permanent: true } => format!("{} scar", kind.name()),
            HediffKind::Injury { kind, .. } => kind.name().to_string(),
            HediffKind::Missing => "Destroyed".into(),
            HediffKind::Infection { .. } => "Infection".into(),
            HediffKind::Flu { .. } => "Flu".into(),
            HediffKind::FoodPoisoning => "Food poisoning".into(),
            HediffKind::Prosthetic { efficiency } => format!("Prosthetic ({:.0}%)", efficiency * 100.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capacities {
    pub consciousness: f32,
    pub moving: f32,
    pub manipulation: f32,
    pub sight: f32,
    pub hearing: f32,
    pub talking: f32,
    pub breathing: f32,
    pub blood_filtration: f32,
    pub blood_pumping: f32,
}

impl Capacities {
    pub fn list(&self) -> [(&'static str, f32); 9] {
        [
            ("Consciousness", self.consciousness),
            ("Moving", self.moving),
            ("Manipulation", self.manipulation),
            ("Sight", self.sight),
            ("Hearing", self.hearing),
            ("Talking", self.talking),
            ("Breathing", self.breathing),
            ("Blood filtration", self.blood_filtration),
            ("Blood pumping", self.blood_pumping),
        ]
    }
}

/// Where a blow landed.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub part: Part,
    pub destroyed: bool,
}

/// Circumstances that affect healing.
#[derive(Clone, Copy, Debug)]
pub struct TickCtx {
    pub in_bed: bool,
    pub resting: bool,
    pub fed: bool,
    /// Wounds heal this much faster (a hospital bed).
    pub care: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Health {
    pub hediffs: Vec<Hediff>,
    /// 0..1 of total blood; fatal at 1.
    pub blood_loss: f32,
    /// 0..1; fatal at 1.
    pub malnutrition: f32,
    /// Derived from the hediffs by `recompute`.
    #[serde(skip)]
    pub caps: Capacities,
    #[serde(skip)]
    pub pain: f32,
}

impl Default for Health {
    fn default() -> Self {
        Health::new()
    }
}

fn pick(options: &[(Part, f32)], rng: &mut Rng) -> Option<Part> {
    let total: f32 = options.iter().map(|o| o.1).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.f32() * total;
    for &(p, w) in options {
        if r < w {
            return Some(p);
        }
        r -= w;
    }
    options.last().map(|o| o.0)
}

impl Health {
    pub fn new() -> Health {
        let mut h = Health { hediffs: Vec::new(), blood_loss: 0.0, malnutrition: 0.0, caps: Capacities::default(), pain: 0.0 };
        h.recompute();
        h
    }

    // ---------- body state ----------

    pub fn is_missing(&self, part: Part) -> bool {
        self.hediffs
            .iter()
            .any(|h| h.kind == HediffKind::Missing && h.part.map_or(false, |m| part.is_within(m)))
    }

    pub fn part_hp(&self, part: Part) -> f32 {
        if self.is_missing(part) {
            return 0.0;
        }
        let damage: f32 = self
            .hediffs
            .iter()
            .filter(|h| h.part == Some(part) && matches!(h.kind, HediffKind::Injury { .. }))
            .map(|h| h.severity.max(0.0))
            .sum();
        (part.def().max_hp - damage).max(0.0)
    }

    pub fn efficiency(&self, part: Part) -> f32 {
        self.part_hp(part) / part.def().max_hp * self.prosthetic_efficiency(part)
    }

    /// How well a prosthetic on `part` (or a part it sits on) works; 1 without one.
    fn prosthetic_efficiency(&self, part: Part) -> f32 {
        self.hediffs
            .iter()
            .find_map(|h| match h.kind {
                HediffKind::Prosthetic { efficiency } if h.part.is_some_and(|p| part.is_within(p)) => Some(efficiency),
                _ => None,
            })
            .unwrap_or(1.0)
    }

    /// Lost parts a prosthetic could replace: each where the loss begins
    /// (a lost arm, not also its hand).
    pub fn lost_parts(&self) -> Vec<Part> {
        self.hediffs
            .iter()
            .filter(|h| h.kind == HediffKind::Missing)
            .filter_map(|h| h.part)
            .filter(|p| !p.def().parent.is_some_and(|parent| self.is_missing(parent)))
            .collect()
    }

    /// Replace the lost `part` (and everything on it) with a prosthetic.
    /// Returns false if it isn't lost.
    pub fn fit_prosthetic(&mut self, part: Part, efficiency: f32) -> bool {
        if !self.lost_parts().contains(&part) {
            return false;
        }
        self.hediffs.retain(|h| !h.part.is_some_and(|p| p.is_within(part)));
        self.hediffs.push(Hediff::new(HediffKind::Prosthetic { efficiency }, Some(part), 0.0));
        self.recompute();
        true
    }

    /// Overall condition 0..1, for health bars.
    pub fn summary(&self) -> f32 {
        let total: f32 = Part::ALL.iter().map(|p| p.def().max_hp).sum();
        let have: f32 = Part::ALL.iter().map(|p| self.part_hp(*p)).sum();
        let disease = self.hediffs.iter().filter(|h| h.is_disease()).map(|h| h.severity).fold(0.0, f32::max);
        (have / total).min(1.0 - self.blood_loss).min(1.0 - self.malnutrition).min(1.0 - disease * 0.6).clamp(0.0, 1.0)
    }

    /// Fraction of blood lost per day.
    pub fn bleed_rate(&self) -> f32 {
        self.hediffs.iter().map(|h| h.bleed()).sum()
    }

    /// Seconds until bleeding out at the current rate, if it's getting worse.
    pub fn time_to_bleed_out(&self) -> Option<f32> {
        let net = self.bleed_rate() - BLOOD_REGEN;
        (net > 0.0).then(|| (1.0 - self.blood_loss) / net * DAY_LEN)
    }

    pub fn needs_tending(&self) -> bool {
        self.hediffs.iter().any(|h| h.needs_tending())
    }

    pub fn has_disease(&self) -> bool {
        self.hediffs.iter().any(|h| h.is_disease())
    }

    /// Should stay in bed: untreated wounds, bleeding, sickness or heavy pain.
    pub fn wants_bed_rest(&self) -> bool {
        self.needs_tending() || self.bleed_rate() > 0.0 || self.blood_loss > 0.25 || self.pain > 0.25 || self.has_disease()
    }

    pub fn downed_reason(&self) -> Option<&'static str> {
        if self.pain >= PAIN_SHOCK {
            Some("pain shock")
        } else if self.caps.consciousness < 0.3 {
            Some("unconscious")
        } else if self.caps.moving < 0.16 {
            Some("unable to move")
        } else {
            None
        }
    }

    pub fn death_cause(&self) -> Option<String> {
        // A vital part can also be lost with its parent (skull -> brain).
        if let Some(p) = Part::ALL.iter().find(|p| p.def().vital && self.is_missing(**p)) {
            return Some(format!("died: {} destroyed", p.name().to_lowercase()));
        }
        for h in &self.hediffs {
            if h.is_disease() && h.severity >= 1.0 {
                return Some(match h.kind {
                    HediffKind::Flu { .. } => "died of the flu".into(),
                    _ => "died of an infection".into(),
                });
            }
        }
        if self.blood_loss >= 1.0 {
            return Some("bled to death".into());
        }
        if self.malnutrition >= 1.0 {
            return Some("starved to death".into());
        }
        if self.caps.breathing <= 0.0 {
            return Some("suffocated".into());
        }
        if self.caps.blood_filtration <= 0.0 {
            return Some("died of organ failure".into());
        }
        None
    }

    /// Re-derive pain and capacities from the current hediffs.
    pub fn recompute(&mut self) {
        let pain = self.hediffs.iter().map(|h| h.pain()).sum::<f32>().min(1.0);
        let worst = |pred: fn(&HediffKind) -> bool| {
            self.hediffs.iter().filter(|h| pred(&h.kind)).map(|h| h.severity).fold(0.0, f32::max)
        };
        let flu = worst(|k| matches!(k, HediffKind::Flu { .. }));
        let infection = worst(|k| matches!(k, HediffKind::Infection { .. }));
        let poisoned = worst(|k| matches!(k, HediffKind::FoodPoisoning));

        let e = |p: Part| self.efficiency(p);
        let pumping = e(Part::Heart);
        let breathing = (e(Part::LeftLung) + e(Part::RightLung)) / 2.0 * (0.5 + 0.5 * e(Part::Neck));
        let filtration = (e(Part::LeftKidney) + e(Part::RightKidney)) / 4.0 + e(Part::Liver) / 2.0;

        let mut con = e(Part::Brain) * (1.0 - 0.35 * pain);
        con -= (self.blood_loss - 0.15).max(0.0) * 1.1;
        con -= 0.3 * self.malnutrition + 0.1 * flu + 0.25 * infection + 0.15 * poisoned;
        con *= (0.4 + 0.6 * pumping) * (0.4 + 0.6 * breathing) * (0.7 + 0.3 * filtration);
        let con = con.clamp(0.0, 1.0);

        let arm_eff = |shoulder: Part, arm: Part, hand: Part| {
            let a = e(arm);
            if a <= 0.0 {
                0.0
            } else {
                0.2 * e(shoulder) + 0.3 * a + 0.5 * e(hand)
            }
        };
        let leg_eff = |leg: Part, foot: Part| {
            let l = e(leg);
            if l <= 0.0 {
                0.0
            } else {
                0.6 * l + 0.4 * e(foot)
            }
        };
        let arms = (arm_eff(Part::LeftShoulder, Part::LeftArm, Part::LeftHand)
            + arm_eff(Part::RightShoulder, Part::RightArm, Part::RightHand))
            / 2.0;
        let legs = (leg_eff(Part::LeftLeg, Part::LeftFoot) + leg_eff(Part::RightLeg, Part::RightFoot)) / 2.0;
        let spine = e(Part::Spine);
        let spine_factor = if spine <= 0.0 { 0.0 } else { 0.5 + 0.5 * spine };

        let manipulation = (arms * con - 0.05 * flu - 0.1 * poisoned).max(0.0);
        let moving = (legs * spine_factor * con * (0.5 + 0.5 * pumping) * (0.5 + 0.5 * breathing)
            - 0.1 * flu
            - 0.2 * poisoned
            - 0.3 * self.malnutrition)
            .max(0.0);
        let caps = Capacities {
            consciousness: con,
            moving,
            manipulation,
            sight: (e(Part::LeftEye) + e(Part::RightEye)) / 2.0,
            hearing: (e(Part::LeftEar) + e(Part::RightEar)) / 2.0,
            talking: e(Part::Jaw) * con,
            breathing,
            blood_filtration: filtration,
            blood_pumping: pumping,
        };
        self.pain = pain;
        self.caps = caps;
    }

    // ---------- damage ----------

    fn pick_outer(&self, rng: &mut Rng) -> Option<Part> {
        let options: Vec<(Part, f32)> = Part::ALL
            .iter()
            .filter(|p| !p.def().inside && !self.is_missing(**p))
            .map(|p| (*p, p.def().hit_weight))
            .collect();
        pick(&options, rng)
    }

    fn pick_inner(&self, parent: Part, rng: &mut Rng) -> Option<Part> {
        let options: Vec<(Part, f32)> = Part::ALL
            .iter()
            .filter(|p| {
                let def = p.def();
                def.inside && def.parent == Some(parent) && !self.is_missing(**p)
            })
            .map(|p| (*p, p.def().hit_weight))
            .collect();
        pick(&options, rng)
    }

    /// Remove a part, everything attached to it, and their conditions.
    fn destroy(&mut self, part: Part) {
        self.hediffs.retain(|h| h.part.map_or(true, |p| !p.is_within(part)));
        self.hediffs.push(Hediff::new(HediffKind::Missing, Some(part), part.def().max_hp));
    }

    /// Wound a specific part. Returns true if the part was destroyed.
    pub fn injure(&mut self, part: Part, amount: f32, kind: DamageKind, rng: &mut Rng) -> bool {
        if self.is_missing(part) || amount <= 0.0 {
            return false;
        }
        if amount >= self.part_hp(part) {
            self.destroy(part);
            return true;
        }
        let mut h = Hediff::new(HediffKind::Injury { kind, permanent: false }, Some(part), amount);
        if kind.can_infect() {
            h.infection_timer = rng.range_f32(0.3, 0.6) * DAY_LEN;
        }
        self.hediffs.push(h);
        false
    }

    /// A blow from outside: picks where it lands and how deep it goes.
    /// `armor` (0..1) is the share of the damage stopped by what the pawn
    /// wears; blows it takes down to almost nothing glance off.
    pub fn apply_damage(&mut self, amount: f32, kind: DamageKind, armor: f32, rng: &mut Rng) -> Option<Hit> {
        let amount = amount * (1.0 - armor.clamp(0.0, 0.9));
        if amount < 1.0 {
            return None;
        }
        let outer = self.pick_outer(rng)?;
        let mut target = outer;
        for _ in 0..2 {
            if !rng.chance(kind.penetration()) {
                break;
            }
            match self.pick_inner(target, rng) {
                Some(inner) => target = inner,
                None => break,
            }
        }
        if target == outer {
            let destroyed = self.injure(target, amount, kind, rng);
            return Some(Hit { part: target, destroyed });
        }
        // Deep hit: the organ takes most of it, the outer layer some on the way in.
        let destroyed = self.injure(target, amount * 0.8, kind, rng);
        self.injure(outer, (amount * 0.3).max(1.0), kind, rng);
        Some(Hit { part: target, destroyed })
    }

    /// Food poisoning, or a fresh bout of it.
    pub fn add_food_poisoning(&mut self) {
        match self.hediffs.iter_mut().find(|h| h.kind == HediffKind::FoodPoisoning) {
            Some(h) => h.severity = 1.0,
            None => self.hediffs.push(Hediff::new(HediffKind::FoodPoisoning, None, 1.0)),
        }
    }

    pub fn add_flu(&mut self) {
        if !self.hediffs.iter().any(|h| matches!(h.kind, HediffKind::Flu { .. })) {
            self.hediffs.push(Hediff::new(HediffKind::Flu { immunity: 0.0 }, None, 0.05));
        }
    }

    // ---------- treatment ----------

    /// Treat up to `max` conditions (bleeding first). Returns how many were tended.
    pub fn tend(&mut self, quality: f32, max: usize) -> usize {
        let mut order: Vec<usize> = (0..self.hediffs.len()).filter(|&i| self.hediffs[i].needs_tending()).collect();
        order.sort_by(|&a, &b| {
            let (ha, hb) = (&self.hediffs[a], &self.hediffs[b]);
            hb.bleed().total_cmp(&ha.bleed()).then(hb.severity.total_cmp(&ha.severity))
        });
        order.truncate(max);
        for &i in &order {
            let h = &mut self.hediffs[i];
            h.tend = Some(quality.clamp(0.0, 1.0));
            if h.is_disease() {
                h.tend_left = DISEASE_TEND_TIME;
            }
        }
        order.len()
    }

    // ---------- time ----------

    /// Advance bleeding, healing, infections and diseases. Returns log events
    /// (to be prefixed with the pawn's name). Call `recompute` afterwards.
    pub fn tick(&mut self, dt: f32, ctx: TickCtx, rng: &mut Rng) -> Vec<String> {
        let days = dt / DAY_LEN;
        let mut events = Vec::new();

        self.blood_loss = (self.blood_loss + (self.bleed_rate() - BLOOD_REGEN) * days).max(0.0);
        if ctx.fed {
            self.malnutrition = (self.malnutrition - 0.3 * days).max(0.0);
        } else {
            self.malnutrition += 0.6 * days;
        }
        let nourished = if self.malnutrition > 0.3 { 0.6 } else { 1.0 };
        let heal = nourished * ctx.care * if ctx.in_bed { 1.6 } else if ctx.resting { 1.2 } else { 1.0 };
        let immunity_gain = nourished * if ctx.in_bed { 1.2 } else { 1.0 };

        let mut infections: Vec<Part> = Vec::new();
        for h in self.hediffs.iter_mut() {
            h.age += dt;
            if h.is_disease() && h.tend.is_some() {
                h.tend_left -= dt;
                if h.tend_left <= 0.0 {
                    h.tend = None;
                }
            }
            let tended = h.tend.is_some();
            let quality = h.tend.unwrap_or(0.0);
            match &mut h.kind {
                HediffKind::Injury { kind, permanent: false } => {
                    // Untended wounds don't close (bruises fade on their own).
                    let rate = if tended {
                        8.0 + 14.0 * quality
                    } else if *kind == DamageKind::Bruise {
                        4.0
                    } else {
                        0.0
                    };
                    h.severity -= rate * heal * days;
                    if h.infection_timer > 0.0 {
                        h.infection_timer -= dt;
                        if h.infection_timer <= 0.0 {
                            let chance = if tended { 0.3 * (1.0 - quality) } else { 0.45 };
                            if let (true, Some(p)) = (rng.chance(chance), h.part) {
                                infections.push(p);
                            }
                        }
                    }
                }
                HediffKind::Infection { immunity } => {
                    h.severity += (0.7 - 0.6 * quality) * days;
                    *immunity += 0.5 * immunity_gain * days;
                }
                HediffKind::Flu { immunity } => {
                    h.severity += (0.3 - 0.25 * quality) * days;
                    *immunity += 0.32 * immunity_gain * days;
                }
                // Passes in under a day.
                HediffKind::FoodPoisoning => h.severity -= 1.3 * days,
                _ => {}
            }
        }

        // Healed wounds may leave scars; diseases beaten by immunity go away.
        let mut i = 0;
        while i < self.hediffs.len() {
            let h = self.hediffs[i];
            let where_ = h.part.map_or("body".to_string(), |p| p.name().to_lowercase());
            match h.kind {
                HediffKind::Injury { kind, permanent: false } if h.severity <= 0.0 => {
                    let delicate = matches!(h.part, Some(Part::LeftEye | Part::RightEye | Part::Brain));
                    let chance = kind.scar_chance() * if delicate { 3.0 } else { 1.0 };
                    if h.peak >= 5.0 && rng.chance(chance) {
                        let severity = (h.peak * 0.25).max(1.0);
                        self.hediffs[i] = Hediff {
                            kind: HediffKind::Injury { kind, permanent: true },
                            severity,
                            peak: severity,
                            tend: None,
                            infection_timer: -1.0,
                            ..h
                        };
                        events.push(format!("will carry a scar on their {}.", where_));
                        i += 1;
                    } else {
                        self.hediffs.remove(i);
                    }
                }
                HediffKind::FoodPoisoning if h.severity <= 0.0 => {
                    events.push("has recovered from food poisoning.".to_string());
                    self.hediffs.remove(i);
                }
                HediffKind::Infection { immunity } | HediffKind::Flu { immunity } if immunity >= 1.0 => {
                    events.push(format!("has fought off the {}.", h.label().to_lowercase()));
                    self.hediffs.remove(i);
                }
                _ => i += 1,
            }
        }

        for part in infections {
            let already = self
                .hediffs
                .iter()
                .any(|h| matches!(h.kind, HediffKind::Infection { .. }) && h.part == Some(part));
            if !already && !self.is_missing(part) {
                self.hediffs.push(Hediff::new(HediffKind::Infection { immunity: 0.0 }, Some(part), 0.05));
                events.push(format!("has an infection in their {}.", part.name().to_lowercase()));
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(h: &mut Health, secs: f32, ctx: TickCtx, rng: &mut Rng) {
        let dt = 0.5;
        for _ in 0..(secs / dt) as usize {
            h.tick(dt, ctx, rng);
            h.recompute();
        }
    }

    const AWAKE: TickCtx = TickCtx { in_bed: false, resting: false, fed: true, care: 1.0 };

    #[test]
    fn healthy_body_has_full_capacities() {
        let h = Health::new();
        for (name, v) in h.caps.list() {
            assert!((v - 1.0).abs() < 1e-4, "{} = {}", name, v);
        }
        assert_eq!(h.summary(), 1.0);
        assert!(h.downed_reason().is_none());
        assert!(h.death_cause().is_none());
    }

    #[test]
    fn gunshots_bleed_until_tended() {
        let mut rng = Rng::new(1);
        let mut h = Health::new();
        h.injure(Part::LeftLeg, 12.0, DamageKind::Gunshot, &mut rng);
        h.recompute();
        assert!(h.bleed_rate() > 0.0);
        assert!(h.needs_tending());
        assert!(h.part_hp(Part::LeftLeg) < 30.0);
        assert_eq!(h.tend(0.8, 3), 1);
        assert_eq!(h.bleed_rate(), 0.0);
        assert!(!h.needs_tending());
    }

    #[test]
    fn losing_a_leg_takes_the_foot_and_slows_walking() {
        let mut rng = Rng::new(1);
        let mut h = Health::new();
        h.injure(Part::LeftFoot, 5.0, DamageKind::Cut, &mut rng);
        assert!(h.injure(Part::LeftLeg, 100.0, DamageKind::Cut, &mut rng));
        h.recompute();
        assert!(h.is_missing(Part::LeftFoot));
        assert!(!h.hediffs.iter().any(|x| x.part == Some(Part::LeftFoot)), "foot wound went with the leg");
        assert!(h.caps.moving < 0.6 && h.caps.moving > 0.3, "moving = {}", h.caps.moving);
        assert!(h.death_cause().is_none());
    }

    #[test]
    fn destroying_the_heart_is_fatal() {
        let mut rng = Rng::new(1);
        let mut h = Health::new();
        h.injure(Part::Heart, 50.0, DamageKind::Gunshot, &mut rng);
        h.recompute();
        assert!(h.death_cause().is_some());
    }

    #[test]
    fn heavy_wounds_cause_pain_shock() {
        let mut rng = Rng::new(1);
        let mut h = Health::new();
        for part in [Part::Torso, Part::LeftArm, Part::RightArm, Part::LeftLeg, Part::RightLeg] {
            h.injure(part, 12.0, DamageKind::Bruise, &mut rng);
        }
        h.recompute();
        assert!(h.pain >= PAIN_SHOCK, "pain = {}", h.pain);
        assert_eq!(h.downed_reason(), Some("pain shock"));
    }

    #[test]
    fn untended_bleeding_kills_and_tending_saves() {
        let mut rng = Rng::new(3);
        let mut bleeding = Health::new();
        for part in [Part::Torso, Part::LeftLeg, Part::RightArm] {
            bleeding.injure(part, 14.0, DamageKind::Cut, &mut rng);
        }
        let mut tended = bleeding.clone();
        tended.tend(0.7, 3);
        // Ignore infections for this test.
        for h in tended.hediffs.iter_mut().chain(bleeding.hediffs.iter_mut()) {
            h.infection_timer = -1.0;
        }
        run(&mut bleeding, 2.0 * DAY_LEN, AWAKE, &mut rng);
        run(&mut tended, 2.0 * DAY_LEN, AWAKE, &mut rng);
        assert_eq!(bleeding.death_cause().as_deref(), Some("bled to death"));
        assert!(tended.death_cause().is_none());
    }

    #[test]
    fn tended_wounds_heal() {
        let mut rng = Rng::new(5);
        let mut h = Health::new();
        h.injure(Part::LeftArm, 10.0, DamageKind::Bruise, &mut rng);
        h.tend(1.0, 1);
        run(&mut h, DAY_LEN, AWAKE, &mut rng);
        assert!(h.hediffs.is_empty());
        assert!(h.summary() > 0.99);
    }

    #[test]
    fn treated_flu_is_survived() {
        let mut rng = Rng::new(9);
        let mut h = Health::new();
        h.add_flu();
        let ctx = TickCtx { in_bed: true, resting: true, fed: true, care: 1.0 };
        for _ in 0..12 {
            h.tend(0.8, 1);
            run(&mut h, DISEASE_TEND_TIME, ctx, &mut rng);
            if h.death_cause().is_some() {
                break;
            }
        }
        assert!(h.death_cause().is_none());
        assert!(!h.has_disease(), "immunity should have won");
    }

    #[test]
    fn starving_leads_to_malnutrition() {
        let mut rng = Rng::new(1);
        let mut h = Health::new();
        let hungry = TickCtx { in_bed: false, resting: false, fed: false, care: 1.0 };
        run(&mut h, DAY_LEN, hungry, &mut rng);
        assert!(h.malnutrition > 0.5);
        assert!(h.caps.consciousness < 1.0);
    }

    #[test]
    fn deep_hits_can_reach_organs() {
        let mut rng = Rng::new(11);
        let mut organ_hits = 0;
        for _ in 0..200 {
            let mut h = Health::new();
            if let Some(hit) = h.apply_damage(5.0, DamageKind::Gunshot, 0.0, &mut rng) {
                if hit.part.def().inside {
                    organ_hits += 1;
                }
            }
        }
        assert!(organ_hits > 20 && organ_hits < 180, "organ hits: {}", organ_hits);
    }

    #[test]
    fn food_poisoning_hurts_then_passes() {
        let mut rng = Rng::new(4);
        let mut h = Health::new();
        h.add_food_poisoning();
        h.recompute();
        assert!(h.caps.consciousness < 0.9 && h.caps.moving < 0.85 && h.pain > 0.1);
        assert!(!h.needs_tending() && h.downed_reason().is_none(), "unpleasant, not dangerous");
        run(&mut h, DAY_LEN, AWAKE, &mut rng);
        assert!(h.hediffs.is_empty());
    }

    #[test]
    fn armour_stops_damage() {
        let total = |armor: f32| {
            let mut rng = Rng::new(8);
            let mut sum = 0.0;
            for _ in 0..200 {
                let mut h = Health::new();
                h.apply_damage(8.0, DamageKind::Cut, armor, &mut rng);
                sum += h.hediffs.iter().map(|d| d.severity).sum::<f32>();
            }
            sum
        };
        let (bare, armoured) = (total(0.0), total(0.5));
        assert!(armoured < bare * 0.6, "{} vs {}", armoured, bare);
        assert_eq!(total(0.9), 0.0, "light blows glance off heavy armour");
    }
}
