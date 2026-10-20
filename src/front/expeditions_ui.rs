//! The Expeditions window: missions on offer (plan one: pick the squad, pack
//! goods), squads away (their journey, the decisions waiting, the market),
//! and the neighbouring factions.

use super::sim::Sim;
use super::theme;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use my_world::defs::ItemId;
use my_world::expeditions::{Expedition, Faction, Stage};
use my_world::map::Stack;
use my_world::pawn::{Pawn, Role, Skill};
use my_world::squads::{fights_at_range, style_name};
use my_world::world::World as SimWorld;
use my_world::DAY_LEN;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Offers,
    Away,
    Factions,
}

/// An expedition being planned: the offer, who goes, what they pack.
pub struct Plan {
    offer: u32,
    squad: Vec<u32>,
    pack: BTreeMap<ItemId, u32>,
}

/// What's being haggled over at a market (for one expedition).
#[derive(Default)]
pub struct Haggle {
    expedition: u32,
    sell: BTreeMap<ItemId, u32>,
    buy: BTreeMap<ItemId, u32>,
}

#[derive(Default)]
pub struct ExpeditionsUi {
    pub open: bool,
    pub tab: Tab,
    plan: Option<Plan>,
    haggle: Haggle,
    /// Why the last thing asked for couldn't be done.
    error: Option<String>,
}

const WARN: Color32 = theme::AMBER;
const GOOD: Color32 = theme::GREEN;

fn hours(secs: f32) -> String {
    let h = secs.max(0.0) / DAY_LEN * 24.0;
    if h >= 36.0 {
        format!("{:.1} days", h / 24.0)
    } else {
        format!("{:.0}h", h.max(1.0))
    }
}

fn goods(list: &[Stack]) -> String {
    if list.is_empty() {
        return "nothing".into();
    }
    list.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect::<Vec<_>>().join(", ")
}

fn stacks(map: &BTreeMap<ItemId, u32>) -> Vec<Stack> {
    map.iter().filter(|(_, &n)| n > 0).map(|(&kind, &count)| Stack { kind, count }).collect()
}

fn worth(list: &[Stack]) -> f32 {
    list.iter().map(|s| s.count as f32 * s.kind.def().value).sum()
}

fn goodwill_color(g: i32) -> Color32 {
    match g {
        g if g >= 15 => GOOD,
        g if g > -15 => theme::WARN,
        _ => WARN,
    }
}

/// Decisions and markets waiting, for the button's label.
pub fn waiting(w: &SimWorld) -> usize {
    w.expeditions.iter().filter(|e| matches!(e.stage, Stage::Decide { .. } | Stage::Trading { .. })).count()
}

pub fn window(ctx: &egui::Context, sim: &mut Sim, ui_state: &mut ExpeditionsUi) {
    let mut open = ui_state.open;
    theme::FieldWindow::new("Expeditions")
        .open(&mut open)
        .anchor(Align2::RIGHT_TOP, [-8.0, 92.0])
        .default_width(460.0)
        .show(ctx, |ui| {
            if sim.home.is_some() {
                ui.label(RichText::new("A squad is fighting at an expedition's site. Win or lose, they carry on afterwards.").color(WARN));
                return;
            }
            let st = &mut *ui_state;
            ui.horizontal(|ui| {
                let away = sim.world.expeditions.len();
                let wait = waiting(&sim.world);
                theme::tab_value(ui, &mut st.tab, Tab::Offers, format!("Offers ({})", sim.world.offers.len()));
                let label = if wait > 0 { format!("Under way ({}, {} waiting)", away, wait) } else { format!("Under way ({})", away) };
                theme::tab_value(ui, &mut st.tab, Tab::Away, label);
                theme::tab_value(ui, &mut st.tab, Tab::Factions, "Factions");
            });
            ui.separator();
            if let Some(e) = &st.error {
                ui.label(RichText::new(e).color(WARN));
            }
            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| match st.tab {
                Tab::Offers => offers(ui, sim, st),
                Tab::Away => away(ui, sim, st),
                Tab::Factions => factions(ui, &sim.world.factions),
            });
        });
    ui_state.open = open;
}

fn offers(ui: &mut egui::Ui, sim: &mut Sim, st: &mut ExpeditionsUi) {
    if st.plan.as_ref().is_some_and(|p| !sim.world.offers.iter().any(|o| o.id == p.offer)) {
        st.plan = None;
    }
    if st.plan.is_some() {
        planning(ui, sim, st);
        return;
    }
    let w = &sim.world;
    if w.offers.is_empty() {
        ui.label(RichText::new("Nothing on offer. New missions come about once a day.").weak());
    }
    for o in &w.offers {
        let f = &w.factions[o.faction];
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(theme::caps(o.kind.name(), 15.0).color(theme::CREAM_BRIGHT));
                ui.label(RichText::new(format!("{} ({})", f.name, f.standing())).color(goodwill_color(f.goodwill)));
            });
            ui.label(o.kind.about());
            ui.label(
                RichText::new(format!("{:.1} days each way · risk: {} · open for {}", o.days, o.kind.risk(), hours(o.until - w.time)))
                    .small()
                    .weak(),
            );
            if ui.button("Plan expedition").clicked() {
                st.plan = Some(Plan { offer: o.id, squad: Vec::new(), pack: BTreeMap::new() });
                st.error = None;
            }
        });
    }
}

fn planning(ui: &mut egui::Ui, sim: &mut Sim, st: &mut ExpeditionsUi) {
    let w = &sim.world;
    let plan = st.plan.as_mut().unwrap();
    let Some(o) = w.offers.iter().find(|o| o.id == plan.offer) else { return };
    let kind = o.kind;
    theme::section(ui, format!("{}: {}", kind.name(), w.factions[o.faction].name));
    ui.label(kind.about());
    ui.add_space(4.0);

    theme::section(ui, "Squad");
    ui.label(RichText::new(kind.squad_hint()).small().weak());
    squad_picker(ui, w, &mut plan.squad);
    ui.add_space(4.0);
    let packed = stacks(&plan.pack);
    let can = my_world::expeditions::carry_capacity(plan.squad.iter().filter_map(|&id| w.pawn(id)));
    let load: u32 = packed.iter().map(|s| s.count).sum();
    theme::section(ui, format!("Pack (worth {:.0}): {} of {} they can carry", worth(&packed), load, can));
    let each = format!("Each carries {} things; backpacks carry more. Loot that doesn't fit is left behind.", my_world::pawn::CARRY_EACH);
    ui.label(RichText::new(each).small().weak());
    if kind.packs() {
        ui.label(RichText::new("Whatever they pack can be traded, given or paid on the way.").small().weak());
    } else {
        ui.label(RichText::new("This mission doesn't need goods (anything packed comes back).").small().weak());
    }
    let totals = w.item_totals();
    egui::Grid::new("pack").num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
        for k in ItemId::all().filter(|k| totals[k.index()] > 0) {
            let n = plan.pack.entry(k).or_insert(0);
            ui.label(k.name());
            ui.label(RichText::new(format!("of {} (worth {:.1} each)", totals[k.index()], k.def().value)).small().weak());
            ui.add(egui::DragValue::new(n).range(0..=totals[k.index()]));
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    let mut send = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        let ready = !plan.squad.is_empty() && (kind != my_world::expeditions::MissionKind::Trade || !packed.is_empty()) && load <= can;
        send = ui.add_enabled(ready, egui::Button::new("Send them")).on_disabled_hover_text("Pick who goes (and, to trade, what to sell), and no more than they can carry").clicked();
        cancel = ui.button("Back").clicked();
    });
    if send {
        let (offer, squad) = (plan.offer, plan.squad.clone());
        match sim.world.send_expedition(offer, &squad, &packed) {
            Ok(()) => {
                st.plan = None;
                st.error = None;
                st.tab = Tab::Away;
            }
            Err(e) => st.error = Some(e),
        }
    } else if cancel {
        st.plan = None;
        st.error = None;
    }
}

/// The colony's squads: each leader with their members.
fn squads(w: &SimWorld) -> Vec<(&Pawn, Vec<&Pawn>)> {
    let colonists: Vec<&Pawn> = w.pawns.iter().filter(|p| p.is_living_colonist()).collect();
    colonists
        .iter()
        .filter(|p| p.role == Role::Leader)
        .map(|&l| (l, colonists.iter().copied().filter(|p| p.role == Role::Member(l.id)).collect()))
        .collect()
}

fn pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// Who goes: whole squads with one click, or colonists one by one, with what
/// matters on the road and in a fight.
fn squad_picker(ui: &mut egui::Ui, w: &SimWorld, picked: &mut Vec<u32>) {
    let squads = squads(w);
    ui.horizontal_wrapped(|ui| {
        for (l, members) in &squads {
            let ids: Vec<u32> = std::iter::once(*l).chain(members.iter().copied()).filter(|p| p.active()).map(|p| p.id).collect();
            let all = !ids.is_empty() && ids.iter().all(|id| picked.contains(id));
            let text = format!("{}'s squad ({}, {})", l.name, ids.len(), style_name(fights_at_range(l)));
            if ui.add_enabled(!ids.is_empty(), egui::Button::new(text).selected(all)).on_hover_text("Click to add the whole squad, again to take it off").clicked() {
                if all {
                    picked.retain(|id| !ids.contains(id));
                } else {
                    picked.extend(ids.into_iter().filter(|id| !picked.contains(id)).collect::<Vec<_>>());
                }
            }
        }
        if squads.is_empty() {
            ui.label(RichText::new("No squads (set them up in a colonist's Combat tab).").small().weak());
        }
        if !picked.is_empty() && ui.button("Nobody").clicked() {
            picked.clear();
        }
    });
    let squad_of = |p: &Pawn| match p.role {
        Role::Leader => "leader".to_string(),
        Role::Member(l) => w.pawn(l).map_or(String::new(), |l| l.name.clone()),
        Role::Civilian => "civilian".to_string(),
        _ => String::new(),
    };
    egui::Grid::new("squad_pick").num_columns(8).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
        for h in ["", "Squad", "Weapon", "Shoot", "Melee", "Armour", "Moving", "Health"] {
            ui.label(theme::caps(h, 12.5).color(theme::CREAM_DIM));
        }
        ui.end_row();
        for p in w.pawns.iter().filter(|p| p.is_living_colonist()) {
            let able = p.active();
            let mut going = picked.contains(&p.id);
            let skills: Vec<String> = Skill::ALL.iter().map(|&s| format!("{} {}", s.name(), p.skill(s))).collect();
            let check = ui.add_enabled(able, egui::Checkbox::new(&mut going, &p.name)).on_hover_text(skills.join("\n"));
            if check.changed() {
                if going {
                    picked.push(p.id);
                } else {
                    picked.retain(|&id| id != p.id);
                }
            }
            ui.label(RichText::new(squad_of(p)).small());
            ui.label(RichText::new(p.weapon_name()).small());
            ui.label(p.skill(Skill::Shooting).to_string());
            ui.label(p.skill(Skill::Melee).to_string());
            ui.label(pct(p.armor()));
            ui.label(pct(p.pace()));
            let hurt = p.health.hediffs.iter().filter(|d| !d.is_disease()).count();
            let (text, color) = if !able {
                ("can't travel".to_string(), WARN)
            } else if hurt > 0 || p.health.hediffs.iter().any(|d| d.is_disease()) {
                (format!("hurt or ill ({})", hurt.max(1)), theme::WARN)
            } else {
                ("fine".to_string(), GOOD)
            };
            ui.label(RichText::new(text).small().color(color));
            ui.end_row();
        }
    });
    let going: Vec<&Pawn> = picked.iter().filter_map(|&id| w.pawn(id)).collect();
    if !going.is_empty() {
        let ranged = going.iter().filter(|p| fights_at_range(p)).count();
        let best = going.iter().map(|p| p.skill(Skill::Shooting).max(p.skill(Skill::Melee))).max().unwrap_or(0);
        ui.label(RichText::new(format!("{} going: {} ranged, {} melee; best fighter skill {}", going.len(), ranged, going.len() - ranged, best)).small());
    }
}

fn stage_line(w: &SimWorld, e: &Expedition) -> String {
    match &e.stage {
        Stage::Out { until, .. } => format!("On the road, something ahead in {}", hours(until - w.time)),
        Stage::Decide { .. } => "Waiting for your decision".into(),
        Stage::Trading { .. } => "At the market".into(),
        Stage::Fighting { .. } => "Fighting".into(),
        Stage::Back { until } => format!("Heading home, back in {}", hours(until - w.time)),
    }
}

fn away(ui: &mut egui::Ui, sim: &mut Sim, st: &mut ExpeditionsUi) {
    if sim.world.expeditions.is_empty() {
        ui.label(RichText::new("Nobody is away.").weak());
    }
    let mut chosen: Option<(u32, usize)> = None;
    let mut leave = None;
    for e in &sim.world.expeditions {
        let f = &sim.world.factions[e.faction];
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(theme::caps(e.kind.name(), 15.0).color(theme::CREAM_BRIGHT));
                ui.label(RichText::new(&f.name).weak());
            });
            let people: Vec<String> = e
                .members
                .iter()
                .map(|p| if p.downed { format!("{} (downed)", p.name) } else if p.health.hediffs.is_empty() { p.name.clone() } else { format!("{} (hurt)", p.name) })
                .collect();
            ui.label(people.join(", "));
            ui.label(RichText::new(stage_line(&sim.world, e)).color(if matches!(e.stage, Stage::Decide { .. } | Stage::Trading { .. }) { WARN } else { Color32::LIGHT_GRAY }));
            ui.label(RichText::new(format!("Carrying {} (worth {:.0}; {} of {})", goods(&e.cargo), e.worth(), e.load(), e.capacity())).small());
            match &e.stage {
                Stage::Decide { event, .. } => {
                    ui.add_space(2.0);
                    ui.label(RichText::new(&event.text).italics());
                    for (c, choice) in event.choices.iter().enumerate() {
                        let ok = e.affords(choice);
                        let b = ui.add_enabled(ok, egui::Button::new(&choice.label)).on_disabled_hover_text("They don't carry enough for that");
                        if b.clicked() {
                            chosen = Some((e.id, c));
                        }
                    }
                }
                Stage::Trading { stock } => {
                    if st.haggle.expedition != e.id {
                        st.haggle = Haggle { expedition: e.id, ..Default::default() };
                    }
                    if market(ui, &sim.world.factions[e.faction], e, stock, &mut st.haggle) {
                        let (sell, buy) = (stacks(&st.haggle.sell), stacks(&st.haggle.buy));
                        chosen = None;
                        leave = Some((e.id, Some((sell, buy))));
                    }
                    if ui.button("Leave the market and head home").clicked() {
                        leave = Some((e.id, None));
                    }
                }
                _ => {}
            }
            egui::CollapsingHeader::new("Journal").id_salt(("journal", e.id)).show(ui, |ui| {
                for line in &e.journal {
                    ui.label(RichText::new(line).small());
                }
            });
        });
    }
    if let Some((id, c)) = chosen {
        st.error = sim.world.choose(id, c).err();
    }
    match leave {
        Some((id, Some((sell, buy)))) => {
            st.error = sim.world.trade(id, &sell, &buy).err();
            if st.error.is_none() {
                st.haggle = Haggle { expedition: id, ..Default::default() };
            }
        }
        Some((id, None)) => {
            sim.world.leave_market(id);
            st.error = None;
        }
        None => {}
    }
}

/// The trading screen: what you carry and what they sell, at their prices.
/// True when the deal is struck.
fn market(ui: &mut egui::Ui, f: &Faction, e: &Expedition, stock: &[Stack], h: &mut Haggle) -> bool {
    let (sf, bf) = (f.sell_factor(), f.buy_factor());
    ui.add_space(2.0);
    ui.label(RichText::new(format!("They pay {:.0}% of an item's worth and ask {:.0}%.", sf * 100.0, bf * 100.0)).small().weak());
    ui.columns(2, |cols| {
        theme::section(&mut cols[0], "You sell");
        egui::Grid::new(("sell", e.id)).num_columns(3).show(&mut cols[0], |ui| {
            for s in &e.cargo {
                let n = h.sell.entry(s.kind).or_insert(0);
                ui.label(format!("{} ({})", s.kind.name(), s.count));
                ui.label(RichText::new(format!("{:.1}", s.kind.def().value * sf)).small().weak());
                ui.add(egui::DragValue::new(n).range(0..=s.count));
                ui.end_row();
            }
        });
        theme::section(&mut cols[1], "You buy");
        egui::Grid::new(("buy", e.id)).num_columns(3).show(&mut cols[1], |ui| {
            for s in stock {
                let n = h.buy.entry(s.kind).or_insert(0);
                ui.label(format!("{} ({})", s.kind.name(), s.count));
                ui.label(RichText::new(format!("{:.1}", s.kind.def().value * bf)).small().weak());
                ui.add(egui::DragValue::new(n).range(0..=s.count));
                ui.end_row();
            }
        });
    });
    let paid = worth(&stacks(&h.sell)) * sf;
    let cost = worth(&stacks(&h.buy)) * bf;
    let fair = paid + 0.01 >= cost && (paid > 0.0 || cost > 0.0);
    ui.horizontal(|ui| {
        ui.label(format!("Your goods: {:.0}   Theirs: {:.0}", paid, cost));
        if paid + 0.01 < cost {
            ui.label(RichText::new(format!("(they want {:.0} more)", cost - paid)).color(WARN));
        } else if paid > cost + 1.0 {
            ui.label(RichText::new(format!("({:.0} given away)", paid - cost)).weak());
        }
    });
    ui.add_enabled(fair, egui::Button::new("Make the deal")).clicked()
}

fn factions(ui: &mut egui::Ui, factions: &[Faction]) {
    for f in factions {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(theme::caps(&f.name, 15.0).color(theme::CREAM_BRIGHT));
                ui.label(RichText::new(format!("{:?}", f.kind)).weak());
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Goodwill {:+} ({})", f.goodwill, f.standing())).color(goodwill_color(f.goodwill)));
                ui.add(theme::bar((f.goodwill + 100) as f32 / 200.0).desired_width(140.0));
            });
            ui.label(
                RichText::new(format!("{:.0} days away · they pay {:.0}% and ask {:.0}% of worth", f.days, f.sell_factor() * 100.0, f.buy_factor() * 100.0))
                    .small()
                    .weak(),
            );
        });
    }
    ui.label(RichText::new("The friendlier the raiders, the longer between raids. Allies (goodwill 50+) send gifts now and then.").small().weak());
}
