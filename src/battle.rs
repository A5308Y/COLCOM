//! Turn-based battles (phase 12), in the style of MENACE.
//!
//! A raid opens a parley (`Parley`): pay the raiders' demand, or fight. A
//! battle stops the whole colony. Units are squads (a leader and the
//! colonists assigned to them, see `squads.rs`), lone fighters, raider
//! squads, and civilians sheltering in the safe zone (who never act but can
//! be attacked). The sides take turns activating one unit each; every unit
//! acts once per round, spending action points on moving, attacking and
//! hunkering down. Each round lets `TURN_SECS` of time pass for wounds and
//! hunger. Squad members who were far from their leader arrive a few
//! rounds in, already in their combat gear.

use crate::defs::ItemId;
use crate::map::{adjacent, Pos, Stack, Zone};
use crate::pathing::bfs;
use crate::pawn::{Faction, Memory, Pawn, Role, Skill};
use crate::world::{Effect, Step, World};
use serde::{Deserialize, Serialize};

/// Action points per unit and round, and what things cost.
pub const AP_MAX: i32 = 100;
pub const AP_MOVE: i32 = 10;
pub const AP_ATTACK: i32 = 40;
pub const AP_HUNKER: i32 = 20;
/// Game seconds that pass per round (for bleeding, healing and hunger).
pub const TURN_SECS: f32 = 6.0;
/// Cells a squad member covers per round on the way to their leader (as
/// far as a unit can run in a round).
pub const CELLS_PER_ROUND: usize = (AP_MAX / AP_MOVE) as usize;
/// Members within this many cells of their leader start the battle with them.
const WITH_LEADER: usize = 2;
/// Suppression from which a unit is pinned down (less AP, worse aim).
pub const PINNED: f32 = 50.0;
/// Morale below which a unit breaks: raiders flee, colonists cower.
pub const BROKEN: f32 = 25.0;
/// Largest raider squad.
const RAIDER_SQUAD: usize = 4;
/// An attack: seconds to take aim, between one member's shot and the next
/// (longer for blows), and to let the last one sink in.
const AIM_TIME: f32 = 0.6;
const SHOT_GAP: f32 = 0.45;
const MELEE_GAP: f32 = 0.8;
const AFTER_ATTACK: f32 = 0.7;
/// Rounds without an attack and without the sides getting any closer
/// after which the raiders give up.
pub const STALEMATE: u32 = 10;
/// What a raid demands per raider, in item value.
pub const DEMAND_PER_RAIDER: f32 = 40.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Cover {
    None,
    Half,
    Full,
}

impl Cover {
    pub fn name(self) -> &'static str {
        match self {
            Cover::None => "no cover",
            Cover::Half => "half cover",
            Cover::Full => "full cover",
        }
    }

    fn weaker(self) -> Cover {
        match self {
            Cover::Full => Cover::Half,
            _ => Cover::None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub side: Faction,
    /// Pawn ids still fighting; a squad's leader comes first.
    pub members: Vec<u32>,
    /// A squad (led by `members[0]`) rather than a lone fighter.
    pub led: bool,
    /// Everyone who has fought in it, the fallen too (to show losses).
    #[serde(default)]
    pub roster: Vec<u32>,
    pub ap: i32,
    /// AP at the start of this round (switching units is allowed until some is spent).
    pub ap_start: i32,
    pub acted: bool,
    pub moved: bool,
    pub hunkered: bool,
    pub suppression: f32,
    pub morale: f32,
    /// Civilians: never act, but can be attacked.
    pub passive: bool,
}

impl Unit {
    fn new(side: Faction, members: Vec<u32>, led: bool, morale: f32) -> Unit {
        Unit {
            side,
            roster: members.clone(),
            members,
            led,
            ap: AP_MAX,
            ap_start: AP_MAX,
            acted: false,
            moved: false,
            hunkered: false,
            suppression: 0.0,
            morale,
            passive: false,
        }
    }

    /// Nobody left in it.
    pub fn out(&self) -> bool {
        self.members.is_empty()
    }

    pub fn pinned(&self) -> bool {
        self.suppression >= PINNED
    }

    pub fn broken(&self) -> bool {
        self.morale < BROKEN
    }

    /// Takes part in the turn order.
    pub fn can_act(&self) -> bool {
        !self.out() && !self.passive
    }
}

/// Someone walking to their unit's new cell (a squad's members all go
/// together, at the pace of the slowest).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Mover {
    pub pawn: u32,
    pub goal: Pos,
}

/// A shot or blow still to come in an attack, `delay` seconds from now.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Strike {
    pub attacker: u32,
    /// Who they aim at to begin with (the actual target is picked when it fires).
    pub target: u32,
    /// The unit attacked.
    #[serde(default)]
    pub unit: usize,
    pub delay: f32,
}

/// A squad member on the way to their leader.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arrival {
    pub pawn: u32,
    pub leader: u32,
    pub rounds: u32,
}

/// A raid waiting for the player's answer: pay the demand, or fight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parley {
    /// Raiders per squad.
    pub squads: Vec<usize>,
    pub entry: Pos,
    /// What they want; empty if the colony doesn't have enough.
    pub demand: Vec<Stack>,
}

impl Parley {
    pub fn raiders(&self) -> usize {
        self.squads.iter().sum()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Won,
    Lost,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Battle {
    pub round: u32,
    pub units: Vec<Unit>,
    pub arrivals: Vec<Arrival>,
    pub side_to_act: Faction,
    /// The unit taking its activation.
    pub active: Option<usize>,
    /// Seconds the last attack still plays out.
    pub busy: f32,
    /// Pawns walking to their unit's new cell.
    pub moving: Vec<Mover>,
    /// What the raiders came for (taken if they win).
    pub demand: Vec<Stack>,
    /// The AI plays the colony's units too (for tests).
    pub auto_colony: bool,
    /// What happened, newest last.
    pub events: Vec<String>,
    /// Rounds in a row without an attack or the sides closing in; raiders
    /// give up after `STALEMATE`.
    #[serde(default)]
    pub quiet_rounds: u32,
    /// Closest the sides have been (cells), to tell closing in from a stalemate.
    #[serde(default)]
    pub closest: Option<usize>,
    /// The attack playing out: shots and blows still to come.
    #[serde(default)]
    pub strikes: Vec<Strike>,
}

impl Battle {
    /// Waiting for an attack or a move to play out.
    pub fn busy(&self) -> bool {
        self.busy > 0.0 || !self.moving.is_empty() || !self.strikes.is_empty()
    }

    /// The unit pawn `id` fights in.
    pub fn unit_of(&self, id: u32) -> Option<usize> {
        self.units.iter().position(|u| u.members.contains(&id))
    }

    fn has_turn_left(&self, side: Faction) -> bool {
        self.units.iter().any(|u| u.side == side && u.can_act() && !u.acted)
    }
}

/// An order for the unit whose turn it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BattleOrder {
    /// Pick which unit acts (before it spends any AP).
    Activate(usize),
    /// Walk the unit (its leader) to a cell; members gather around it.
    Move(Pos),
    /// Shoot at, or strike, a unit: everyone in the unit who can reach it.
    Attack(usize),
    /// Take cover for the rest of the round (ends the activation).
    Hunker,
    End,
}

fn dist(a: &Pawn, b: &Pawn) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

fn place(p: &mut Pawn, c: Pos) {
    p.pos = c;
    (p.x, p.y, p.z) = (c.0 as f32, c.1 as f32, c.2 as f32);
    p.path.clear();
    p.path_goal = None;
}

fn other(side: Faction) -> Faction {
    match side {
        Faction::Colony => Faction::Raider,
        Faction::Raider => Faction::Colony,
    }
}

impl World {
    // ---------- parley ----------

    /// Goods worth about `value` from what's lying in the colony, most
    /// valuable first; empty if there isn't that much.
    pub fn goods_worth(&self, value: f32) -> Vec<Stack> {
        let totals = self.item_totals();
        let mut kinds: Vec<ItemId> = ItemId::all().filter(|k| totals[k.index()] > 0).collect();
        kinds.sort_by(|a, b| b.def().value.total_cmp(&a.def().value));
        let mut left = value;
        let mut out = Vec::new();
        for k in kinds {
            if left <= 0.0 {
                break;
            }
            let v = k.def().value.max(0.01);
            let have = self.map.count_items(k);
            let count = ((left / v).ceil() as u32).min(have);
            if count > 0 {
                out.push(Stack { kind: k, count });
                left -= count as f32 * v;
            }
        }
        if left > 0.0 {
            Vec::new()
        } else {
            out
        }
    }

    /// Remove goods from wherever they lie in the colony.
    fn take_goods(&mut self, goods: &[Stack]) {
        for g in goods {
            let mut left = g.count;
            for i in 0..self.map.len() {
                if left == 0 {
                    break;
                }
                if let Some(s) = self.map.take_item(i, g.kind, left) {
                    left -= s.count;
                }
            }
        }
    }

    /// Raiders arrive and make their demand; the colony stops until answered.
    pub(crate) fn open_parley(&mut self, raiders: usize, entry: Pos) {
        let mut squads = Vec::new();
        let mut left = raiders;
        while left > 0 {
            let n = left.min(RAIDER_SQUAD);
            squads.push(n);
            left -= n;
        }
        let demand = self.goods_worth(raiders as f32 * DEMAND_PER_RAIDER);
        self.parley = Some(Parley { squads, entry, demand });
    }

    /// Hand over what the raiders demand; they leave. False if there's
    /// nothing to pay with (they want a fight).
    pub fn pay_tribute(&mut self) -> bool {
        let Some(p) = self.parley.take() else { return false };
        if p.demand.is_empty() {
            self.parley = Some(p);
            return false;
        }
        self.take_goods(&p.demand);
        let paid: Vec<String> = p.demand.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect();
        self.push_log(format!("You paid the raiders {} and they left.", paid.join(", ")));
        true
    }

    /// Turn the raiders down: battle.
    pub fn refuse_tribute(&mut self) {
        if let Some(p) = self.parley.take() {
            self.start_battle(p);
        }
    }

    // ---------- starting and ending ----------

    pub(crate) fn start_battle(&mut self, parley: Parley) {
        let mut b = Battle {
            round: 1,
            units: Vec::new(),
            arrivals: Vec::new(),
            side_to_act: Faction::Colony,
            active: None,
            busy: 0.0,
            moving: Vec::new(),
            demand: parley.demand.clone(),
            auto_colony: false,
            events: Vec::new(),
            quiet_rounds: 0,
            closest: None,
            strikes: Vec::new(),
        };
        // Everyone drops what they're doing.
        for i in 0..self.pawns.len() {
            if self.pawns[i].is_colonist() && !self.pawns[i].dead {
                let mut p = self.pawns[i].clone();
                self.end_job(&mut p);
                self.pawns[i] = p;
            }
        }

        // Raiders come in squads near where they entered.
        let df = bfs(&self.map, parley.entry);
        // Each squad stands on one cell, a few cells from the next.
        let spots: Vec<Pos> = df.order.iter().map(|&i| self.map.pos(i)).collect();
        for (k, &size) in parley.squads.iter().enumerate() {
            let Some(&at) = spots.get(k * 6).or(spots.last()) else { break };
            let mut members = Vec::new();
            for _ in 0..size {
                let name = crate::story::raider_name(&mut self.rng);
                members.push(self.spawn_raider(name, at));
            }
            if !members.is_empty() {
                let led = members.len() > 1;
                b.units.push(Unit::new(Faction::Raider, members, led, 60.0));
            }
        }

        // The colony: squads around their leaders, lone fighters where they stand.
        let ready = |p: &Pawn| p.is_colonist() && p.active();
        let colonists: Vec<(u32, Role, Pos)> =
            self.pawns.iter().filter(|p| ready(p)).map(|p| (p.id, p.role, p.pos)).collect();
        let leaders: Vec<(u32, Pos)> =
            colonists.iter().filter(|c| c.1 == Role::Leader).map(|c| (c.0, c.2)).collect();
        let mut fighters = Vec::new();
        for &(leader, at) in &leaders {
            let df = bfs(&self.map, at);
            let mut members = vec![leader];
            for &(id, role, pos) in &colonists {
                if role != Role::Member(leader) {
                    continue;
                }
                let steps = df.dist(pos).map_or(pos.0.abs_diff(at.0) + pos.1.abs_diff(at.1), |d| d as usize);
                if steps <= WITH_LEADER {
                    // A squad holds one cell: the leader's.
                    let i = self.pawn_index(id).unwrap();
                    place(&mut self.pawns[i], at);
                    members.push(id);
                } else {
                    let rounds = steps.saturating_sub(WITH_LEADER).div_ceil(CELLS_PER_ROUND).max(1) as u32;
                    b.arrivals.push(Arrival { pawn: id, leader, rounds });
                }
                fighters.push(id);
            }
            fighters.push(leader);
            let morale = self.unit_morale(&members) + 10.0;
            b.units.push(Unit::new(Faction::Colony, members, true, morale));
        }
        for &(id, role, _) in &colonists {
            let alone = match role {
                Role::Fighter => true,
                // A member whose leader can't fight fights alone.
                Role::Member(l) => !leaders.iter().any(|&(lid, _)| lid == l),
                _ => false,
            };
            if alone {
                let morale = self.unit_morale(&[id]);
                b.units.push(Unit::new(Faction::Colony, vec![id], false, morale));
                fighters.push(id);
            }
        }
        // Civilians run to the safe zone (if there is one) and keep their heads down.
        let mut safe: Vec<Pos> =
            (0..self.map.len()).filter(|&i| self.map.zone[i] == Zone::Safe).map(|i| self.map.pos(i)).collect();
        safe.retain(|&c| self.map.standable(c));
        for &(id, role, pos) in &colonists {
            if role != Role::Civilian {
                continue;
            }
            if let Some(k) = (0..safe.len()).min_by_key(|&k| safe[k].0.abs_diff(pos.0) + safe[k].1.abs_diff(pos.1)) {
                let c = safe.swap_remove(k);
                let i = self.pawn_index(id).unwrap();
                place(&mut self.pawns[i], c);
            }
            let mut u = Unit::new(Faction::Colony, vec![id], false, 50.0);
            u.passive = true;
            u.hunkered = true;
            b.units.push(u);
        }
        for id in fighters {
            self.gear_up(id);
        }
        // Everyone runs while the battle lasts.
        for p in self.pawns.iter_mut() {
            p.running = true;
        }
        let raiders = parley.raiders();
        let who = if raiders == 1 { "1 raider attacks".to_string() } else { format!("{} raiders attack", raiders) };
        b.events.push(format!("Battle! {}.", who));
        self.battle = Some(b);
        self.sound("raid", None);
        self.push_log(format!("Battle! {} the colony.", who));
    }

    /// Starting morale: how the members feel.
    fn unit_morale(&self, members: &[u32]) -> f32 {
        let moods: Vec<f32> = members.iter().filter_map(|&id| self.pawn(id)).map(|p| p.mood).collect();
        let avg = if moods.is_empty() { 50.0 } else { moods.iter().sum::<f32>() / moods.len() as f32 };
        avg.clamp(35.0, 85.0)
    }

    /// Put on the combat loadout, taking what's missing from storage.
    fn gear_up(&mut self, id: u32) {
        let Some(i) = self.pawn_index(id) else { return };
        let loadout = self.pawns[i].loadout.clone();
        for kind in loadout {
            let p = &self.pawns[i];
            if p.weapon == Some(kind) || p.apparel.contains(&kind) {
                continue;
            }
            let at = p.pos;
            let nearest = (0..self.map.len())
                .filter(|&c| self.map.items[c].is_some_and(|s| s.kind == kind) && self.res_item[c] == 0)
                .min_by_key(|&c| {
                    let q = self.map.pos(c);
                    q.0.abs_diff(at.0) + q.1.abs_diff(at.1) + q.2.abs_diff(at.2)
                });
            let Some(c) = nearest else {
                let msg = format!("{} has no {} to fight with.", self.pawns[i].name, kind.name().to_lowercase());
                self.push_log(msg);
                continue;
            };
            if self.map.take_item(c, kind, 1).is_some() {
                let prev = self.pawns[i].wear(kind);
                self.pawns[i].stash.push((kind, prev));
            }
        }
    }

    /// Take the battle gear off (left at their feet for haulers) and put
    /// their own things back on.
    fn gear_down(&mut self, id: u32) {
        let Some(i) = self.pawn_index(id) else { return };
        let stash: Vec<(ItemId, Option<ItemId>)> = self.pawns[i].stash.drain(..).rev().collect();
        for (taken, prev) in stash {
            let p = &mut self.pawns[i];
            let at = p.pos;
            if p.take_off(taken) {
                self.map.drop_item(at, Stack { kind: taken, count: 1 });
            }
            if let Some(prev) = prev {
                if let Some(extra) = self.pawns[i].wear(prev) {
                    self.map.drop_item(at, Stack { kind: extra, count: 1 });
                }
            }
        }
    }

    fn end_battle(&mut self, outcome: Outcome) {
        let Some(b) = self.battle.take() else { return };
        for p in self.pawns.iter_mut() {
            p.running = false;
        }
        let colonists: Vec<u32> = self.pawns.iter().filter(|p| p.is_colonist() && !p.dead).map(|p| p.id).collect();
        for &id in &colonists {
            self.gear_down(id);
        }
        let memory = match outcome {
            Outcome::Won => Memory::WonBattle,
            Outcome::Lost => Memory::LostBattle,
        };
        for p in self.pawns.iter_mut().filter(|p| p.is_colonist() && !p.dead) {
            p.add_memory(memory);
        }
        match outcome {
            Outcome::Won => self.push_log(format!("The raid was beaten off after {} rounds.", b.round)),
            Outcome::Lost => {
                let loot = self.goods_worth(b.demand.iter().map(|s| s.count as f32 * s.kind.def().value).sum::<f32>() * 1.5);
                self.take_goods(&loot);
                let taken: Vec<String> = loot.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect();
                let what = if taken.is_empty() { "what they could find".to_string() } else { taken.join(", ") };
                self.push_log(format!("The raiders won. They took {} and left.", what));
            }
        }
        // Raiders still standing leave; the downed stay where they fell.
        for p in self.pawns.iter_mut().filter(|p| !p.is_colonist() && p.active()) {
            p.left = true;
        }
        self.pawns.retain(|p| p.is_colonist() || !p.left);
    }

    // ---------- the turn order ----------

    /// The player's order for the colony's unit; refused when it isn't the
    /// colony's turn or the order doesn't make sense.
    pub fn battle_order(&mut self, order: BattleOrder) -> Result<(), String> {
        let b = self.battle.as_ref().ok_or("there's no battle")?;
        if b.side_to_act != Faction::Colony || b.auto_colony {
            return Err("it's the raiders' turn".into());
        }
        self.do_order(order)
    }

    pub(crate) fn do_order(&mut self, order: BattleOrder) -> Result<(), String> {
        let b = self.battle.as_ref().ok_or("there's no battle")?;
        if b.busy() {
            return Err("wait for the last action to finish".into());
        }
        if let BattleOrder::Activate(u) = order {
            let unit = b.units.get(u).ok_or("no such unit")?;
            if unit.side != b.side_to_act || unit.acted || !unit.can_act() {
                return Err("that unit can't act now".into());
            }
            if let Some(a) = b.active {
                if a != u && b.units[a].ap < b.units[a].ap_start {
                    return Err("finish with the active unit first".into());
                }
            }
            let b = self.battle.as_mut().unwrap();
            b.active = Some(u);
            b.units[u].hunkered = false;
            b.units[u].moved = false;
            return Ok(());
        }
        let u = b.active.ok_or("pick a unit first")?;
        let ap = b.units[u].ap;
        match order {
            BattleOrder::Activate(_) => unreachable!(),
            BattleOrder::End => {
                self.end_activation();
                Ok(())
            }
            BattleOrder::Hunker => {
                if ap < AP_HUNKER {
                    return Err("not enough action points".into());
                }
                let b = self.battle.as_mut().unwrap();
                b.units[u].hunkered = true;
                b.units[u].ap = 0;
                self.end_activation();
                Ok(())
            }
            BattleOrder::Move(to) => {
                let cost = self.move_cost(u, to)?;
                if cost > ap {
                    return Err("too far for the action points left".into());
                }
                self.start_move(u, to);
                let b = self.battle.as_mut().unwrap();
                b.units[u].ap -= cost;
                b.units[u].moved = true;
                Ok(())
            }
            BattleOrder::Attack(t) => {
                if ap < AP_ATTACK {
                    return Err("not enough action points".into());
                }
                self.attack_unit(u, t)?;
                // The attack may have ended the battle.
                if let Some(b) = self.battle.as_mut() {
                    b.units[u].ap -= AP_ATTACK;
                }
                Ok(())
            }
        }
    }

    /// AP to walk unit `u` to `to`, if it can stand there.
    pub fn move_cost(&self, u: usize, to: Pos) -> Result<i32, String> {
        let b = self.battle.as_ref().ok_or("there's no battle")?;
        let unit = b.units.get(u).ok_or("no such unit")?;
        let lead = self.pawn(*unit.members.first().ok_or("no one left")?).ok_or("no one left")?;
        if !self.map.standable(to) {
            return Err("can't stand there".into());
        }
        if self.pawns.iter().any(|p| p.active() && p.pos == to && !unit.members.contains(&p.id)) {
            return Err("someone is standing there".into());
        }
        let d = bfs(&self.map, lead.pos).dist(to).ok_or("can't get there")?;
        if d == 0 {
            return Err("already there".into());
        }
        Ok(d * AP_MOVE)
    }

    /// The whole unit sets off for `to` together.
    fn start_move(&mut self, u: usize, to: Pos) {
        let members = self.battle.as_ref().unwrap().units[u].members.clone();
        let moving = members.iter().map(|&pawn| Mover { pawn, goal: to }).collect();
        self.battle.as_mut().unwrap().moving = moving;
    }

    /// The cell unit `u` holds (its leader's).
    pub fn unit_cell(&self, u: usize) -> Option<Pos> {
        let b = self.battle.as_ref()?;
        self.pawn(*b.units.get(u)?.members.first()?).map(|p| p.pos)
    }

    /// Who in unit `u` can attack whom in unit `t`: (attacker, target) pawn ids.
    pub fn attack_options(&self, u: usize, t: usize) -> Vec<(u32, u32)> {
        let Some(b) = &self.battle else { return Vec::new() };
        let (Some(unit), Some(target)) = (b.units.get(u), b.units.get(t)) else { return Vec::new() };
        if unit.side == target.side {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &m in &unit.members {
            let Some(a) = self.pawn(m) else { continue };
            for &tm in &target.members {
                let Some(o) = self.pawn(tm) else { continue };
                if self.can_attack(a, o) {
                    out.push((m, tm));
                }
            }
        }
        out
    }

    /// In reach: within range and in sight for guns, next to them for blows.
    pub fn can_attack(&self, a: &Pawn, o: &Pawn) -> bool {
        let w = a.attack();
        if w.ranged {
            dist(a, o) <= w.range && battle_sight(self, a.pos, o.pos)
        } else {
            adjacent(a.pos, o.pos) && a.pos.2 == o.pos.2
        }
    }

    /// Line up an attack: each member who can reach someone in unit `t`
    /// takes aim, then they fire (or strike) one after another.
    fn attack_unit(&mut self, u: usize, t: usize) -> Result<(), String> {
        let options = self.attack_options(u, t);
        if options.is_empty() {
            return Err("out of reach (range, sight, or not next to them)".into());
        }
        let mut attackers: Vec<u32> = options.iter().map(|o| o.0).collect();
        attackers.dedup();
        // Shots go round the unit: everyone's first, then the second shots (rifles fire two).
        let shots: Vec<(u32, u32)> = attackers
            .iter()
            .map(|&a| (a, self.pawn(a).and_then(|p| p.weapon).and_then(|k| k.def().weapon.as_ref()).map_or(1, |w| w.shots.max(1))))
            .collect();
        let most = shots.iter().map(|s| s.1).max().unwrap_or(1);
        let mut strikes = Vec::new();
        let mut at = AIM_TIME;
        for round in 0..most {
            for &(a, _) in shots.iter().filter(|s| s.1 > round) {
                let Some(ai) = self.pawn_index(a) else { continue };
                let Some(target) = self.pick_target(ai, t) else { continue };
                strikes.push(Strike { attacker: a, target, unit: t, delay: at });
                let melee = !self.pawns[ai].attack().ranged;
                at += if melee { MELEE_GAP } else { SHOT_GAP };
            }
        }
        let b = self.battle.as_mut().unwrap();
        b.strikes = strikes;
        b.quiet_rounds = 0;
        Ok(())
    }

    /// One shot or blow of an attack. If the target is already down, the
    /// attacker picks someone else from that unit in reach.
    fn fire_strike(&mut self, s: Strike) {
        let Some(ai) = self.pawn_index(s.attacker).filter(|&i| self.pawns[i].active()) else { return };
        let target = self.pick_target(ai, s.unit);
        let Some(ti) = target.and_then(|t| self.pawn_index(t)) else { return };
        let chance = self.hit_chance(&self.pawns[ai], &self.pawns[ti]);
        let hit = self.rng.chance(chance);
        let w = self.pawns[ai].attack();
        let skill = if w.ranged { Skill::Shooting } else { Skill::Melee };
        let (from, to) = ((self.pawns[ai].x, self.pawns[ai].y, self.pawns[ai].z), (self.pawns[ti].x, self.pawns[ti].y, self.pawns[ti].z));
        {
            let p = &mut self.pawns[ai];
            // A jump in cooldown is what the front end animates as a shot or blow.
            p.cooldown = 1.0;
            p.learn(skill, 2.0);
        }
        self.sound(w.sound, Some(self.pawns[ai].pos));
        if w.ranged {
            let to = if hit { to } else { (to.0 + self.rng.range_f32(-1.0, 1.0), to.1 + self.rng.range_f32(-1.0, 1.0), to.2) };
            self.effects.push(Effect { from, to, ttl: 0.15, hit });
        }
        let target_id = self.pawns[ti].id;
        if let Some(b) = self.battle.as_mut() {
            if let Some(tu) = b.unit_of(target_id) {
                b.units[tu].suppression += if hit { 15.0 } else { 8.0 };
            }
        }
        if hit {
            let (lo, hi) = w.damage;
            let dmg = self.rng.range_f32(lo, hi);
            let by = self.pawns[ai].name.clone();
            self.damage(ti, dmg, w.kind, &by);
        }
        self.refresh_units();
    }

    /// Someone in unit `t` the pawn at index `ai` can reach, at random; a
    /// squad's leader only once nobody else in it is left standing.
    fn pick_target(&mut self, ai: usize, t: usize) -> Option<u32> {
        let b = self.battle.as_ref()?;
        let unit = b.units.get(t)?;
        let mut targets: Vec<u32> = unit
            .members
            .iter()
            .copied()
            .filter(|&o| self.pawn(o).is_some_and(|o| o.active() && self.can_attack(&self.pawns[ai], o)))
            .collect();
        if unit.led && targets.len() > 1 {
            let leader = unit.members[0];
            targets.retain(|&o| o != leader);
        }
        (!targets.is_empty()).then(|| targets[self.rng.below(targets.len())])
    }

    /// Who pawn `id` is aiming at in the attack playing out, if anyone.
    pub fn aim_target(&self, id: u32) -> Option<u32> {
        self.battle.as_ref()?.strikes.iter().find(|s| s.attacker == id).map(|s| s.target)
    }

    /// Chance that `a`'s shot or blow hits `o`.
    pub fn hit_chance(&self, a: &Pawn, o: &Pawn) -> f32 {
        let w = a.attack();
        let skill = if w.ranged { Skill::Shooting } else { Skill::Melee };
        let level = a.skill(skill) as f32;
        let d = dist(a, o);
        let mut c = if w.ranged { 0.55 + level * 0.025 - d * 0.025 } else { 0.6 + level * 0.02 };
        let caps = &a.health.caps;
        c *= (0.4 + 0.6 * caps.sight.min(1.0)) * (0.5 + 0.5 * caps.manipulation.min(1.0));
        let b = self.battle.as_ref();
        let unit = |id: u32| b.and_then(|b| b.unit_of(id).map(|u| &b.units[u]));
        if w.ranged {
            c *= match self.cover(a.pos, o.pos) {
                Cover::None => 1.0,
                Cover::Half => 0.7,
                Cover::Full => 0.45,
            };
            if unit(o.id).is_some_and(|u| u.hunkered) {
                c *= 0.75;
            }
            if a.pos.2 > o.pos.2 {
                c *= 1.15;
            }
        }
        if unit(a.id).is_some_and(|u| u.pinned()) {
            c *= 0.7;
        }
        c.clamp(0.05, 0.95)
    }

    /// Cover someone at `at` has from shots coming from `from`: what stands
    /// on the side facing the shooter. Shooting down weakens it.
    pub fn cover(&self, from: Pos, at: Pos) -> Cover {
        let m = &self.map;
        let dx = (from.0 as i32 - at.0 as i32).signum();
        let dy = (from.1 as i32 - at.1 as i32).signum();
        let mut best = Cover::None;
        for (cx, cy) in [(dx, 0), (0, dy)] {
            if (cx, cy) == (0, 0) {
                continue;
            }
            let Some(c) = m.at(at.0 as i32 + cx, at.1 as i32 + cy, at.2 as i32) else { continue };
            best = best.max(self.cover_at(c));
        }
        if from.2 > at.2 {
            best.weaker()
        } else {
            best
        }
    }

    /// How much cover the cell `c` gives: rock, walls and doors fully; any
    /// other structure, and trees, half.
    pub fn cover_at(&self, c: Pos) -> Cover {
        let m = &self.map;
        let i = m.idx(c);
        let f = m.feature[i];
        if m.block[i].solid() || f.blocks_sight() {
            Cover::Full
        } else if f.structure().is_some_and(|d| d.special != crate::defs::Special::Stairs) || !f.passable() {
            Cover::Half
        } else {
            Cover::None
        }
    }

    fn end_activation(&mut self) {
        let Some(b) = self.battle.as_mut() else { return };
        if let Some(u) = b.active.take() {
            b.units[u].acted = true;
        }
        let (side, next) = (b.side_to_act, other(b.side_to_act));
        if b.has_turn_left(next) {
            b.side_to_act = next;
        } else if !b.has_turn_left(side) {
            self.new_round();
        }
    }

    fn new_round(&mut self) {
        {
            let b = self.battle.as_mut().unwrap();
            b.round += 1;
            for u in b.units.iter_mut() {
                u.acted = false;
                u.ap = if u.pinned() { AP_MAX * 6 / 10 } else { AP_MAX };
                u.ap_start = u.ap;
                if !u.pinned() {
                    u.morale = (u.morale + 5.0).min(90.0);
                }
                u.suppression *= 0.5;
            }
            b.side_to_act = Faction::Colony;
            for a in b.arrivals.iter_mut() {
                a.rounds = a.rounds.saturating_sub(1);
            }
            b.quiet_rounds += 1;
        }
        // Closing in counts as progress too.
        let gap = self.gap_between_sides();
        let b = self.battle.as_mut().unwrap();
        if gap.is_some_and(|g| b.closest.is_none_or(|c| g < c)) {
            b.closest = gap;
            b.quiet_rounds = 0;
        }
        if self.battle.as_ref().is_some_and(|b| b.quiet_rounds >= STALEMATE) {
            self.push_log("The raiders gave up and left.".into());
            self.end_battle(Outcome::Won);
            return;
        }
        // Time passes for wounds and hunger.
        self.time += TURN_SECS;
        for i in 0..self.pawns.len() {
            let mut p = self.pawns[i].clone();
            if !p.dead && !p.left {
                self.update_health(&mut p, TURN_SECS);
                if p.is_colonist() {
                    p.hunger = (p.hunger - 0.4 * TURN_SECS).max(0.0);
                    p.rest = (p.rest - 0.33 * TURN_SECS).max(0.0);
                }
            }
            self.pawns[i] = p;
        }
        self.arrive();
        // Wounds may have decided it.
        self.refresh_units();
        if let Some(b) = self.battle.as_mut() {
            if !b.has_turn_left(Faction::Colony) {
                b.side_to_act = Faction::Raider;
            }
        }
    }

    /// Fewest cells between any fighter of the colony and any raider.
    fn gap_between_sides(&self) -> Option<usize> {
        let b = self.battle.as_ref()?;
        let side = |s: Faction| -> Vec<Pos> {
            b.units
                .iter()
                .filter(|u| u.side == s && !u.passive)
                .flat_map(|u| u.members.iter().filter_map(|&id| self.pawn(id)).map(|p| p.pos))
                .collect()
        };
        let (ours, theirs) = (side(Faction::Colony), side(Faction::Raider));
        ours.iter()
            .flat_map(|a| theirs.iter().map(move |b| a.0.abs_diff(b.0) + a.1.abs_diff(b.1) + a.2.abs_diff(b.2)))
            .min()
    }

    /// Squad members whose time is up join their leader.
    fn arrive(&mut self) {
        let b = self.battle.as_mut().unwrap();
        let (due, waiting): (Vec<Arrival>, Vec<Arrival>) = b.arrivals.drain(..).partition(|a| a.rounds == 0);
        b.arrivals = waiting;
        for a in due {
            let Some(pi) = self.pawn_index(a.pawn).filter(|&i| self.pawns[i].active()) else { continue };
            let b = self.battle.as_ref().unwrap();
            let leader_unit = b.unit_of(a.leader).filter(|&u| !b.units[u].out());
            let name = self.pawns[pi].name.clone();
            match leader_unit {
                Some(u) => {
                    let members = b.units[u].members.clone();
                    let at = self.pawn(members[0]).unwrap().pos;
                    place(&mut self.pawns[pi], at);
                    let b = self.battle.as_mut().unwrap();
                    b.units[u].members.push(a.pawn);
                    b.units[u].roster.push(a.pawn);
                    b.units[u].morale = (b.units[u].morale + 5.0).min(90.0);
                    let leader = self.pawn(members[0]).map_or(String::new(), |l| l.name.clone());
                    self.battle_event(format!("{} joins {}'s squad.", name, leader));
                }
                None => {
                    let morale = self.unit_morale(&[a.pawn]);
                    let b = self.battle.as_mut().unwrap();
                    b.units.push(Unit::new(Faction::Colony, vec![a.pawn], false, morale));
                    self.battle_event(format!("{} arrives; their leader is down, so they fight alone.", name));
                }
            }
        }
    }

    fn battle_event(&mut self, msg: String) {
        if let Some(b) = self.battle.as_mut() {
            b.events.push(msg);
            if b.events.len() > 30 {
                b.events.remove(0);
            }
        }
    }

    /// Drop members who are down, dead or gone; losses shake morale, losing
    /// the leader most. Ends the battle when a side has no one left.
    fn refresh_units(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let mut events = Vec::new();
        let mut units = b.units.clone();
        for u in units.iter_mut() {
            let before = u.members.clone();
            let was_broken = u.broken();
            u.members.retain(|&id| self.pawn(id).is_some_and(|p| p.active()));
            let lost = before.len() - u.members.len();
            if lost == 0 {
                continue;
            }
            u.morale -= 20.0 * lost as f32;
            if u.led && !u.members.contains(&before[0]) {
                u.morale -= 20.0;
                if let (Some(l), false) = (self.pawn(before[0]), u.out()) {
                    events.push(format!("{}'s squad lost its leader.", l.name));
                }
            }
            if !was_broken && u.broken() && !u.out() && !u.passive {
                let who = self.pawn(u.members[0]).map_or(String::new(), |p| p.name.clone());
                events.push(match u.side {
                    Faction::Raider => format!("{}'s group loses its nerve and runs.", who),
                    Faction::Colony => format!("{} is shaken and takes cover.", who),
                });
            }
        }
        let b = self.battle.as_mut().unwrap();
        b.units = units;
        for e in events {
            self.battle_event(e);
        }
        self.check_battle_end();
    }

    fn check_battle_end(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let raiders = b.units.iter().any(|u| u.side == Faction::Raider && !u.out());
        let colony = b.units.iter().any(|u| u.side == Faction::Colony && u.can_act()) || !b.arrivals.is_empty();
        if !raiders {
            self.end_battle(Outcome::Won);
        } else if !colony {
            self.end_battle(Outcome::Lost);
        }
    }

    /// Advance a battle: play out moves and attacks, then let the AI act
    /// (or wait for the player).
    pub(crate) fn battle_tick(&mut self, dt: f32) {
        for p in self.pawns.iter_mut() {
            p.cooldown = (p.cooldown - dt).max(0.0);
        }
        let Some(b) = self.battle.as_mut() else { return };
        if b.busy > 0.0 {
            b.busy -= dt;
            return;
        }
        if !b.strikes.is_empty() {
            for s in b.strikes.iter_mut() {
                s.delay -= dt;
            }
            let (due, later): (Vec<Strike>, Vec<Strike>) = b.strikes.drain(..).partition(|s| s.delay <= 0.0);
            b.strikes = later;
            for s in due {
                if self.battle.is_none() {
                    return;
                }
                self.fire_strike(s);
            }
            if let Some(b) = self.battle.as_mut() {
                if b.strikes.is_empty() {
                    b.busy = AFTER_ATTACK;
                }
            }
            return;
        }
        if !b.moving.is_empty() {
            let moving = b.moving.clone();
            // Everyone keeps to the pace of the slowest, so the squad stays together.
            let pace = moving.iter().filter_map(|m| self.pawn(m.pawn)).map(|p| p.move_speed()).fold(f32::MAX, f32::min);
            let mut still = Vec::new();
            for m in moving {
                let Some(i) = self.pawn_index(m.pawn) else { continue };
                let mut p = self.pawns[i].clone();
                let step = dt * pace / p.move_speed().max(0.01);
                if matches!(self.go_to(&mut p, m.goal, false, step), Step::Moving) {
                    still.push(m);
                }
                self.pawns[i] = p;
            }
            let done = still.is_empty();
            self.battle.as_mut().unwrap().moving = still;
            if done {
                self.escape_at_edges();
                self.refresh_units();
            }
            return;
        }
        let b = self.battle.as_ref().unwrap();
        if let Some(u) = b.active {
            if b.units[u].out() || b.units[u].ap < AP_MOVE {
                self.end_activation();
                return;
            }
        }
        let b = self.battle.as_ref().unwrap();
        if b.side_to_act == Faction::Raider || b.auto_colony {
            self.ai_step();
        } else if b.active.is_none() {
            // Shaken colonists sit their turn out on their own.
            let cowering = b
                .units
                .iter()
                .position(|u| u.side == Faction::Colony && u.can_act() && !u.acted && u.broken());
            let others = b.units.iter().any(|u| u.side == Faction::Colony && u.can_act() && !u.acted && !u.broken());
            if let (Some(u), false) = (cowering, others) {
                let _ = self.do_order(BattleOrder::Activate(u));
                let _ = self.do_order(BattleOrder::Hunker);
            }
        }
    }

    /// Fleeing raiders who reach the edge of the map are gone.
    fn escape_at_edges(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let (w, h) = (self.map.w, self.map.h);
        let fleeing: Vec<u32> = b
            .units
            .iter()
            .filter(|u| u.side == Faction::Raider && u.broken())
            .flat_map(|u| u.members.iter().copied())
            .collect();
        for id in fleeing {
            let Some(i) = self.pawn_index(id) else { continue };
            let p = &mut self.pawns[i];
            if p.pos.0 == 0 || p.pos.1 == 0 || p.pos.0 == w - 1 || p.pos.1 == h - 1 {
                p.left = true;
                let name = p.name.clone();
                self.battle_event(format!("{} fled.", name));
            }
        }
    }
}

/// Line of sight for battles: like `pathing::line_of_sight`, but cover right
/// next to either end doesn't block it (you lean out to shoot, and cover
/// protects without hiding you completely).
pub fn battle_sight(w: &World, a: Pos, b: Pos) -> bool {
    let m = &w.map;
    let from = [a.0 as f32 + 0.5, a.1 as f32 + 0.5, a.2 as f32 + 0.6];
    let to = [b.0 as f32 + 0.5, b.1 as f32 + 0.5, b.2 as f32 + 0.6];
    let span = (0..3).map(|k| (to[k] - from[k]).abs()).fold(0.0, f32::max);
    let steps = (span * 3.0).ceil() as usize;
    let near = |p: Pos, e: Pos| p.2 == e.2 && p.0.abs_diff(e.0) <= 1 && p.1.abs_diff(e.1) <= 1;
    for s in 1..steps {
        let t = s as f32 / steps as f32;
        let c: Vec<f32> = (0..3).map(|k| from[k] + (to[k] - from[k]) * t).collect();
        let p = (c[0].floor() as usize, c[1].floor() as usize, c[2].floor() as usize);
        if near(p, a) || near(p, b) {
            continue;
        }
        let i = m.idx(p);
        if m.block[i].solid() || m.feature[i].blocks_sight() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Feature, Map, FLAT_Z as Z};
    use crate::rng::Rng;
    use crate::TICK;

    fn item(id: &str) -> ItemId {
        ItemId::named(id)
    }

    fn world(w: usize, h: usize, seed: u64) -> World {
        let mut world = World::with_map(Map::flat(w, h), Rng::new(seed));
        world.story.next_raid = f32::MAX;
        world.story.next_event = f32::MAX;
        world.regrow_timer = f32::MAX;
        world
    }

    fn colonist(w: &mut World, at: Pos) -> u32 {
        let id = w.spawn_colonist(at);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 100.0;
        w.pawns[i].rest = 100.0;
        w.pawns[i].mood = 60.0;
        id
    }

    fn fight(w: &mut World, raiders: usize, entry: Pos) {
        w.start_battle(Parley { squads: vec![raiders], entry, demand: Vec::new() });
    }

    fn battle(w: &World) -> &Battle {
        w.battle.as_ref().expect("a battle is on")
    }

    /// Tick until the battle is over (or give up).
    fn play_out(w: &mut World, secs: f32) {
        for _ in 0..(secs / TICK) as usize {
            if w.battle.is_none() {
                return;
            }
            w.tick(TICK);
        }
    }

    fn raiders(w: &World) -> Vec<u32> {
        w.pawns.iter().filter(|p| !p.is_colonist()).map(|p| p.id).collect()
    }

    #[test]
    fn paying_the_raiders_sends_them_away() {
        let mut w = world(20, 20, 1);
        colonist(&mut w, (5, 5, Z));
        w.map.drop_item((3, 3, Z), Stack { kind: item("steel"), count: 30 });
        w.map.drop_item((4, 3, Z), Stack { kind: item("wood"), count: 50 });
        w.open_parley(3, (19, 10, Z));
        let demand = w.parley.as_ref().unwrap().demand.clone();
        let value: f32 = demand.iter().map(|s| s.count as f32 * s.kind.def().value).sum();
        assert!(value >= 3.0 * DEMAND_PER_RAIDER, "they want enough: {:?}", demand);
        assert_eq!(demand[0].kind, item("steel"), "the most valuable first");

        // The colony stands still while you decide.
        let t = w.time;
        w.tick(TICK);
        assert_eq!(w.time, t);

        assert!(w.pay_tribute());
        assert!(w.parley.is_none() && w.battle.is_none());
        assert!(raiders(&w).is_empty(), "nobody attacks");
        assert_eq!(w.map.count_items(item("steel")), 30 - demand[0].count);
    }

    #[test]
    fn a_poor_colony_has_to_fight() {
        let mut w = world(20, 20, 1);
        colonist(&mut w, (5, 5, Z));
        w.open_parley(3, (19, 10, Z));
        assert!(w.parley.as_ref().unwrap().demand.is_empty());
        assert!(!w.pay_tribute(), "nothing to pay with");
        w.refuse_tribute();
        assert_eq!(battle(&w).units.iter().filter(|u| u.side == Faction::Raider).count(), 1);
        assert_eq!(raiders(&w).len(), 3);
    }

    #[test]
    fn squads_form_around_their_leaders() {
        let mut w = world(40, 20, 2);
        let leader = colonist(&mut w, (5, 5, Z));
        let near = colonist(&mut w, (6, 5, Z));
        let far = colonist(&mut w, (17, 5, Z));
        let lone = colonist(&mut w, (5, 15, Z));
        let civ = colonist(&mut w, (30, 15, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(near, Role::Member(leader)).unwrap();
        w.set_role(far, Role::Member(leader)).unwrap();
        w.set_role(civ, Role::Civilian).unwrap();
        for x in 2..4 {
            let i = w.map.idx((x, 2, Z));
            w.map.zone[i] = Zone::Safe;
        }
        fight(&mut w, 2, (39, 10, Z));
        let b = battle(&w);
        let squad = &b.units[b.unit_of(leader).unwrap()];
        assert_eq!(squad.members, vec![leader, near], "the near one is with the leader");
        assert_eq!(w.pawn(near).unwrap().pos, w.pawn(leader).unwrap().pos, "a squad holds one cell");
        assert!(squad.led);
        // 12 cells away: 10 beyond the leader's side, run in one round.
        assert_eq!(b.arrivals.len(), 1);
        assert_eq!((b.arrivals[0].pawn, b.arrivals[0].rounds), (far, 1));
        assert_eq!(b.units[b.unit_of(lone).unwrap()].members, vec![lone], "fights alone");
        let civilians = &b.units[b.unit_of(civ).unwrap()];
        assert!(civilians.passive && !civilians.can_act());
        assert_eq!(w.map.zone[w.map.idx(w.pawn(civ).unwrap().pos)], Zone::Safe, "ran to the safe zone");

        // A round later, the far member stands beside the leader.
        w.new_round();
        let b = battle(&w);
        assert!(b.arrivals.is_empty());
        assert!(b.units[b.unit_of(leader).unwrap()].members.contains(&far));
        let (a, l) = (w.pawn(far).unwrap().pos, w.pawn(leader).unwrap().pos);
        assert_eq!(a, l, "arrived on the leader's cell");
    }

    #[test]
    fn combat_gear_comes_from_storage_and_goes_back() {
        let mut w = world(30, 20, 3);
        let id = colonist(&mut w, (5, 5, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].apparel = vec![item("clothes")];
        w.pawns[i].weapon = None;
        w.map.drop_item((8, 8, Z), Stack { kind: item("steel_armor"), count: 1 });
        w.map.drop_item((9, 8, Z), Stack { kind: item("rifle"), count: 1 });
        w.set_loadout(id, &[item("rifle"), item("steel_armor")]);
        fight(&mut w, 1, (29, 10, Z));
        let p = w.pawn(id).unwrap();
        assert_eq!(p.weapon, Some(item("rifle")));
        assert!(p.apparel.contains(&item("steel_armor")) && p.apparel.contains(&item("clothes")));
        assert_eq!(w.map.count_items(item("rifle")) + w.map.count_items(item("steel_armor")), 0, "taken from storage");

        w.end_battle(Outcome::Won);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.weapon, None, "back to how they were");
        assert_eq!(p.apparel, vec![item("clothes")]);
        assert!(p.stash.is_empty());
        assert_eq!(w.map.count_items(item("rifle")) + w.map.count_items(item("steel_armor")), 2, "left for the haulers");
    }

    #[test]
    fn sides_take_turns_one_unit_at_a_time() {
        let mut w = world(40, 20, 4);
        let a = colonist(&mut w, (5, 5, Z));
        let b2 = colonist(&mut w, (5, 15, Z));
        fight(&mut w, 1, (39, 10, Z));
        let unit = |w: &World, id: u32| battle(w).unit_of(id).unwrap();
        let (ua, ub) = (unit(&w, a), unit(&w, b2));
        let raider = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        assert_eq!(battle(&w).side_to_act, Faction::Colony, "the colony goes first");
        assert!(w.battle_order(BattleOrder::Activate(raider)).is_err(), "not their turn");
        w.battle_order(BattleOrder::Activate(ua)).unwrap();
        w.battle_order(BattleOrder::End).unwrap();
        assert_eq!(battle(&w).side_to_act, Faction::Raider);
        assert!(w.battle_order(BattleOrder::Activate(ub)).is_err(), "the raiders are up");
        w.do_order(BattleOrder::Activate(raider)).unwrap();
        w.do_order(BattleOrder::End).unwrap();
        // The raiders are done for the round; the colony plays its other unit.
        assert_eq!(battle(&w).side_to_act, Faction::Colony);
        assert!(w.battle_order(BattleOrder::Activate(ua)).is_err(), "already acted");
        w.battle_order(BattleOrder::Activate(ub)).unwrap();
        let t = w.time;
        w.battle_order(BattleOrder::End).unwrap();
        let b = battle(&w);
        assert_eq!(b.round, 2, "everyone has acted: a new round");
        assert!(b.units.iter().all(|u| !u.acted));
        assert_eq!(w.time, t + TURN_SECS, "time passes per round");
    }

    #[test]
    fn squads_move_together() {
        let mut w = world(40, 20, 16);
        let leader = colonist(&mut w, (5, 10, Z));
        let a = colonist(&mut w, (5, 11, Z));
        let b2 = colonist(&mut w, (6, 10, Z));
        w.set_role(leader, Role::Leader).unwrap();
        for m in [a, b2] {
            w.set_role(m, Role::Member(leader)).unwrap();
        }
        // One of them limps: the squad keeps to their pace.
        let i = w.pawn_index(b2).unwrap();
        w.pawns[i].health.caps.moving = 0.6;
        fight(&mut w, 1, (39, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        assert_eq!(w.unit_cell(u), Some((5, 10, Z)));
        // Another unit's cell can't be shared.
        let lone = colonist(&mut w, (9, 10, Z));
        w.battle.as_mut().unwrap().units.push(Unit::new(Faction::Colony, vec![lone], false, 50.0));
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert!(w.move_cost(u, (9, 10, Z)).is_err(), "taken by another unit");
        w.battle_order(BattleOrder::Move((10, 12, Z))).unwrap();
        for _ in 0..(0.7 / TICK) as usize {
            w.tick(TICK);
            let at = |id: u32| {
                let p = w.pawn(id).unwrap();
                (p.x, p.y)
            };
            let (l, x, y) = (at(leader), at(a), at(b2));
            assert!((l.0 - x.0).abs() + (l.1 - x.1).abs() < 0.01 && (l.0 - y.0).abs() + (l.1 - y.1).abs() < 0.01, "together");
        }
        for _ in 0..(8.0 / TICK) as usize {
            w.tick(TICK);
        }
        for id in [leader, a, b2] {
            assert_eq!(w.pawn(id).unwrap().pos, (10, 12, Z), "everyone on the new cell");
        }
        assert!(!battle(&w).busy());
    }

    #[test]
    fn action_points_limit_what_a_unit_does() {
        let mut w = world(40, 20, 5);
        let id = colonist(&mut w, (5, 10, Z));
        fight(&mut w, 1, (39, 10, Z));
        let u = battle(&w).unit_of(id).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert_eq!(w.move_cost(u, (8, 10, Z)), Ok(3 * AP_MOVE));
        assert!(w.battle_order(BattleOrder::Move((17, 10, Z))).is_err(), "12 cells is more than 100 AP");
        w.battle_order(BattleOrder::Move((8, 10, Z))).unwrap();
        assert!(w.battle_order(BattleOrder::End).is_err(), "busy walking");
        for _ in 0..60 {
            w.tick(TICK);
        }
        assert_eq!(w.pawn(id).unwrap().pos, (8, 10, Z));
        assert_eq!(battle(&w).units[u].ap, AP_MAX - 3 * AP_MOVE);
        let raider = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        assert!(w.battle_order(BattleOrder::Attack(raider)).is_err(), "AP left, but 25 cells is out of range");
        assert!(w.battle_order(BattleOrder::Activate(raider)).is_err());
    }

    #[test]
    fn cover_and_height_change_the_odds() {
        let mut w = world(30, 30, 6);
        let shooter = colonist(&mut w, (5, 15, Z));
        let target = colonist(&mut w, (12, 15, Z));
        let chance = |w: &World| w.hit_chance(w.pawn(shooter).unwrap(), w.pawn(target).unwrap());
        let open = chance(&w);
        let fence = w.map.idx((11, 15, Z));
        w.map.feature[fence] = Feature::built("fence");
        assert_eq!(w.cover((5, 15, Z), (12, 15, Z)), Cover::Half);
        let half = chance(&w);
        w.map.feature[fence] = Feature::built("wall");
        assert_eq!(w.cover((5, 15, Z), (12, 15, Z)), Cover::Full);
        assert!(battle_sight(&w, (5, 15, Z), (12, 15, Z)), "you can still lean out and shoot");
        let full = chance(&w);
        assert!(open > half && half > full, "{} > {} > {}", open, half, full);
        assert_eq!(w.cover((12, 25, Z), (12, 15, Z)), Cover::None, "no cover on the open side");

        // From a level up, the wall only half covers, and height helps aim.
        let i = w.pawn_index(shooter).unwrap();
        w.pawns[i].pos.2 = Z + 1;
        w.pawns[i].z = (Z + 1) as f32;
        assert_eq!(w.cover((5, 15, Z + 1), (12, 15, Z)), Cover::Half);
        assert!(chance(&w) > full);
    }

    #[test]
    fn suppression_pins_and_losses_break_morale() {
        let mut w = world(40, 20, 7);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (6, 10, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        fight(&mut w, 1, (39, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        w.battle.as_mut().unwrap().units[u].suppression = 80.0;
        w.new_round();
        assert_eq!(battle(&w).units[u].ap, AP_MAX * 6 / 10, "pinned: fewer action points");

        let before = battle(&w).units[u].morale;
        let i = w.pawn_index(leader).unwrap();
        let mut p = w.pawns[i].clone();
        w.kill(&mut p, "was shot");
        w.pawns[i] = p;
        w.refresh_units();
        let b = battle(&w);
        assert_eq!(b.units[u].members, vec![member], "the squad fights on");
        assert!(b.units[u].morale <= before - 40.0, "losing the leader hurts");
        assert!(b.events.iter().any(|e| e.contains("lost its leader")));
    }

    #[test]
    fn broken_raiders_run_off_the_map() {
        let mut w = world(30, 12, 8);
        let id = colonist(&mut w, (3, 6, Z));
        fight(&mut w, 1, (27, 6, Z));
        let r = raiders(&w)[0];
        let ru = battle(&w).unit_of(r).unwrap();
        w.battle.as_mut().unwrap().units[ru].morale = 5.0;
        let u = battle(&w).unit_of(id).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::End).unwrap();
        play_out(&mut w, 30.0);
        assert!(w.battle.is_none(), "with the raider gone, the battle is won");
        assert!(w.pawn(r).is_none(), "fled the map");
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::WonBattle));
    }

    #[test]
    fn a_strong_colony_wins() {
        let mut w = world(40, 24, 9);
        for k in 0..3 {
            let id = colonist(&mut w, (6, 9 + 3 * k, Z));
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].skills[Skill::Shooting as usize] = 15;
            w.pawns[i].weapon = Some(item("rifle"));
        }
        fight(&mut w, 1, (30, 12, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("knife"));
        w.battle.as_mut().unwrap().auto_colony = true;
        let t = w.time;
        play_out(&mut w, 300.0);
        assert!(w.battle.is_none(), "over in time");
        assert!(w.pawn(r).is_none_or(|p| !p.active()), "the raider is down, dead or gone");
        assert!(w.time > t, "rounds took time");
    }

    #[test]
    fn a_weak_colony_loses_its_goods() {
        let mut w = world(40, 24, 10);
        let id = colonist(&mut w, (6, 12, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = None;
        w.pawns[i].skills[Skill::Melee as usize] = 0;
        w.map.drop_item((3, 3, Z), Stack { kind: item("steel"), count: 60 });
        w.open_parley(4, (30, 12, Z));
        let demand = w.parley.as_ref().unwrap().demand.clone();
        assert!(!demand.is_empty());
        w.refuse_tribute();
        for r in raiders(&w) {
            let ri = w.pawn_index(r).unwrap();
            w.pawns[ri].weapon = Some(item("rifle"));
            w.pawns[ri].skills[Skill::Shooting as usize] = 15;
        }
        w.battle.as_mut().unwrap().auto_colony = true;
        play_out(&mut w, 600.0);
        assert!(w.battle.is_none(), "over in time");
        assert!(!w.pawn(id).unwrap().active(), "the colonist went down");
        assert!(w.map.count_items(item("steel")) < 60, "the raiders took goods");
        assert!(raiders(&w).iter().all(|&r| !w.pawn(r).unwrap().active()), "the rest left");
    }

    #[test]
    fn raiders_closing_in_from_afar_dont_give_up() {
        let mut w = world(150, 12, 12);
        let id = colonist(&mut w, (2, 6, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("knife"));
        fight(&mut w, 1, (147, 6, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("knife"));
        w.battle.as_mut().unwrap().auto_colony = true;
        play_out(&mut w, 900.0);
        assert!(w.battle.is_none(), "over in time");
        assert!(!w.log.iter().any(|l| l.contains("gave up")), "they kept coming: {:?}", w.log);
    }

    #[test]
    fn raiders_who_cant_get_at_anyone_give_up() {
        let mut w = world(30, 12, 13);
        let id = colonist(&mut w, (4, 6, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("knife"));
        // Walled in, no door: nobody gets in or out.
        for y in 4..=8 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 4 || y == 8 {
                    let c = w.map.idx((x, y, Z));
                    w.map.feature[c] = Feature::built("wall");
                }
            }
        }
        fight(&mut w, 1, (27, 6, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("knife"));
        w.battle.as_mut().unwrap().auto_colony = true;
        play_out(&mut w, 900.0);
        assert!(w.battle.is_none(), "over in time");
        assert!(w.log.iter().any(|l| l.contains("gave up")));
    }

    #[test]
    fn everyone_runs_in_battle_and_walks_after() {
        let mut w = world(30, 12, 14);
        let id = colonist(&mut w, (4, 6, Z));
        assert!(!w.pawn(id).unwrap().running);
        let walk = w.pawn(id).unwrap().move_speed();
        fight(&mut w, 1, (27, 6, Z));
        assert!(w.pawns.iter().all(|p| p.running));
        assert!(w.pawn(id).unwrap().move_speed() > 2.0 * walk);
        w.end_battle(Outcome::Won);
        assert!(w.pawns.iter().all(|p| !p.running));
    }

    #[test]
    fn attacks_play_out_one_shot_after_another() {
        let mut w = world(40, 20, 15);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (5, 11, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        for id in [leader, member] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("rifle"));
        }
        fight(&mut w, 1, (14, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        let raider = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Attack(raider)).unwrap();
        let strikes = battle(&w).strikes.clone();
        assert_eq!(strikes.len(), 4, "rifles fire twice each");
        let order: Vec<u32> = strikes.iter().map(|s| s.attacker).collect();
        assert_eq!(order, vec![leader, member, leader, member], "everyone's first shot, then the second");
        assert!(strikes.windows(2).all(|p| p[1].delay > p[0].delay), "one after the other");
        assert_eq!(w.aim_target(leader), Some(strikes[0].target), "aiming first");
        assert!(w.battle_order(BattleOrder::End).is_err(), "busy while the attack plays out");
        let shots = |w: &mut World| w.take_sounds().iter().filter(|s| s.name == "rifle").count();
        shots(&mut w);
        for _ in 0..(0.3 / TICK) as usize {
            w.tick(TICK);
        }
        assert_eq!(shots(&mut w), 0, "still taking aim");
        let mut fired = Vec::new();
        for k in 0..(3.0 / TICK) as usize {
            if w.battle.is_none() {
                break;
            }
            w.tick(TICK);
            if shots(&mut w) > 0 {
                fired.push(k);
            }
        }
        assert!(!fired.is_empty(), "shots were fired");
        assert!(fired.len() < 2 || fired[1] - fired[0] >= 10, "not at the same moment: {:?}", fired);
        assert!(w.battle.as_ref().is_none_or(|b| b.strikes.is_empty() && !b.busy()), "and it's over");
    }

    #[test]
    fn squad_leaders_are_hit_last_and_losses_are_remembered() {
        let mut w = world(40, 20, 17);
        let shooter = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(shooter).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        fight(&mut w, 3, (14, 10, Z));
        let t = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        let leader = battle(&w).units[t].members[0];
        assert!(battle(&w).units[t].led);
        for _ in 0..200 {
            let picked = w.pick_target(i, t).unwrap();
            assert_ne!(picked, leader, "not the leader while others stand");
        }
        // Down the other two: now the leader is the only target.
        let others: Vec<u32> = battle(&w).units[t].members[1..].to_vec();
        for o in &others {
            let oi = w.pawn_index(*o).unwrap();
            let mut p = w.pawns[oi].clone();
            w.kill(&mut p, "was shot");
            w.pawns[oi] = p;
        }
        w.refresh_units();
        assert_eq!(w.pick_target(i, t), Some(leader));
        let unit = &battle(&w).units[t];
        assert_eq!(unit.members, vec![leader]);
        assert_eq!(unit.roster.len(), 3, "the fallen are still on the roster");
    }

    #[test]
    fn battles_survive_saving_and_loading() {
        let mut w = world(40, 20, 11);
        colonist(&mut w, (5, 10, Z));
        fight(&mut w, 2, (39, 10, Z));
        w.battle.as_mut().unwrap().units[0].suppression = 30.0;
        let loaded = crate::save::from_json(&crate::save::to_json(&w).unwrap()).unwrap();
        let (a, b) = (battle(&w), loaded.battle.as_ref().unwrap());
        assert_eq!(a.units.len(), b.units.len());
        assert_eq!(a.units[0].members, b.units[0].members);
        assert_eq!(b.units[0].suppression, 30.0);
        assert_eq!(a.side_to_act, b.side_to_act);
    }
}
