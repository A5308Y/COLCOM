//! The info window: everything worth knowing about an item, a buildable or a
//! plant (a weapon's damage and range, what a workbench makes, how long a
//! crop takes), with links between related things and a search to look up
//! anything in the game.

use super::input::UiState;
use super::theme;
use bevy_egui::egui::{self, Align2, RichText};
use my_world::defs::{defs, Amount, BuildDef, BuildId, ItemId, PlantId, RecipeId, Slot, Special, UtilityEffect};
use my_world::map::STACK_MAX;
use my_world::world::{DAWN, DUSK, HUNGER_DRAIN};
use my_world::DAY_LEN;

/// What the info window shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoTarget {
    Item(ItemId),
    Build(BuildId),
    Plant(PlantId),
}

impl InfoTarget {
    pub fn name(self) -> &'static str {
        match self {
            InfoTarget::Item(k) => k.name(),
            InfoTarget::Build(k) => k.name(),
            InfoTarget::Plant(k) => k.name(),
        }
    }

    /// Everything that can be looked up (roofs come by themselves, so not those).
    fn all() -> impl Iterator<Item = InfoTarget> {
        let items = ItemId::all().map(InfoTarget::Item);
        let builds = BuildId::all().filter(|k| k.def().special != Special::Roof).map(InfoTarget::Build);
        items.chain(builds).chain(PlantId::all().map(InfoTarget::Plant))
    }
}

/// A stat sheet: rows of label and value under headings, and related things.
pub struct Sheet {
    pub title: String,
    /// What kind of thing it is ("Weapons", "Furniture", "Plant").
    pub kind: String,
    pub about: Vec<String>,
    pub sections: Vec<Section>,
    pub links: Vec<InfoTarget>,
}

pub struct Section {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
}

impl Sheet {
    fn new(title: &str, kind: &str) -> Sheet {
        Sheet { title: title.into(), kind: kind.into(), about: Vec::new(), sections: Vec::new(), links: Vec::new() }
    }

    /// Add a section (left out if it has no rows).
    fn section(&mut self, title: &'static str, rows: Vec<(String, String)>) {
        if !rows.is_empty() {
            self.sections.push(Section { title, rows });
        }
    }

    fn link(&mut self, t: InfoTarget) {
        if !self.links.contains(&t) {
            self.links.push(t);
        }
    }

    /// The value of the row labelled `label`, in any section (for tests).
    #[cfg(test)]
    fn row(&self, label: &str) -> Option<&str> {
        self.sections.iter().flat_map(|s| &s.rows).find(|r| r.0 == label).map(|r| r.1.as_str())
    }
}

fn row(label: &str, value: impl Into<String>) -> (String, String) {
    (label.into(), value.into())
}

fn amounts(list: &[Amount]) -> String {
    let parts: Vec<String> = list.iter().map(|a| format!("{} {}", a.count, a.item.name().to_lowercase())).collect();
    parts.join(", ")
}

/// Game seconds as in-game hours or days.
fn duration(secs: f32) -> String {
    let hours = secs / DAY_LEN * 24.0;
    if hours < 30.0 {
        format!("{:.0} h", hours.max(1.0))
    } else {
        format!("{:.1} days", hours / 24.0)
    }
}

fn plural(n: u32, one: &str) -> String {
    if n == 1 {
        format!("1 {}", one)
    } else {
        format!("{} {}s", n, one)
    }
}

pub fn sheet(t: InfoTarget) -> Sheet {
    match t {
        InfoTarget::Item(k) => item_sheet(k),
        InfoTarget::Build(k) => build_sheet(k),
        InfoTarget::Plant(k) => plant_sheet(k),
    }
}

fn item_sheet(k: ItemId) -> Sheet {
    let d = k.def();
    let mut s = Sheet::new(&d.name, &d.category);
    s.about.extend(d.about.clone());

    let mut general = vec![row("Value", format!("{} each", d.value))];
    if let Some((lo, hi)) = d.pods {
        general.push(row("Cargo pods", format!("sometimes bring {}-{}", lo, hi)));
    }
    s.section("General", general);

    let mut food = Vec::new();
    if d.nutrition > 0.0 {
        let lasts = duration(d.nutrition / HUNGER_DRAIN);
        food.push(row("Nutrition", format!("{:.0}% of a full stomach (lasts about {})", d.nutrition, lasts)));
        if d.raw {
            food.push(row("Raw", "eating it is unpleasant; cook it into meals"));
        }
        if d.poison > 0.0 {
            food.push(row("Food poisoning", format!("{:.0}% chance each", d.poison * 100.0)));
        }
    }
    s.section("Food", food);

    if d.medicine > 0.0 {
        let better = format!("+{:.0}% tending quality", 25.0 * d.medicine);
        s.section("Medicine", vec![row("Used by doctors", better)]);
    }

    if let Some(w) = &d.weapon {
        let mut rows = vec![row("Type", if w.ranged() { "Ranged" } else { "Melee" })];
        if w.sidearm {
            rows.push(row("Sidearm", "light enough to carry besides a main weapon (drawn when it suits better)"));
        }
        if w.ranged() {
            rows.push(row("Range", format!("{:.0} cells", w.range)));
        }
        rows.push(row("Damage", format!("{:.0}-{:.0} per hit ({})", w.damage.0, w.damage.1, w.kind.name().to_lowercase())));
        rows.push(row("Between attacks", format!("{:.1} s", w.cooldown)));
        if w.shots > 1 {
            rows.push(row("In battle", format!("{} per attack", plural(w.shots, "shot"))));
        }
        let dps = (w.damage.0 + w.damage.1) / 2.0 / w.cooldown;
        rows.push(row("Damage per second", format!("about {:.1}", dps)));
        s.section("Weapon", rows);
    }

    if let Some(a) = &d.apparel {
        let slots: Vec<&str> = a.slots().map(Slot::name).collect();
        let mut rows = vec![row("Worn on", slots.join(", "))];
        if a.armor > 0.0 {
            let parts: Vec<&str> = if a.sealed {
                vec!["the whole body"]
            } else {
                let mut parts: Vec<&str> = Vec::new();
                for p in a.slots().flat_map(|s| s.protects().iter()) {
                    if !parts.contains(&p.name()) {
                        parts.push(p.name());
                    }
                }
                parts
            };
            rows.push(row("Armour", format!("stops {:.0}% of each blow landing on {}", a.armor * 100.0, parts.join(", ").to_lowercase())));
        }
        if a.carry > 0 {
            rows.push(row("Carry", format!("+{} things on an expedition", a.carry)));
        }
        if a.tend > 0.0 {
            rows.push(row("Tending", format!("+{:.0}% quality, in battle (medkits) and at home", a.tend * 100.0)));
        }
        if a.ap > 0 {
            rows.push(row("Battle", format!("+{} action points a round for the wearer's unit", a.ap)));
        }
        if a.aim != 0.0 {
            rows.push(row("Aim", format!("{:+.0}% chance to hit", a.aim * 100.0)));
        }
        if a.mood != 0.0 {
            rows.push(row("Mood", format!("{:+.0} while worn", a.mood)));
        }
        if a.warmth > 0.0 {
            rows.push(row("Warmth", format!("+{:.0}°C felt in the cold (none in summer heat)", a.warmth)));
        }
        if a.speed != 1.0 {
            rows.push(row("Speed", format!("{:+.0}% walking, and in battle per action point", (a.speed - 1.0) * 100.0)));
        }
        s.section("Apparel", rows);
    }

    if let Some(c) = &d.cyberware {
        let slots: Vec<&str> = c.fits.iter().map(|s| s.name()).collect();
        let replaces = c.fits.iter().any(|s| s.replaces());
        let mut rows = vec![row("Slot", slots.join(" or "))];
        if replaces {
            rows.push(row("Works", format!("{:.0}% as well as a healthy natural part", c.efficiency * 100.0)));
        } else {
            rows.push(row("Adds to", "the body (nothing natural is replaced)"));
        }
        if c.armor > 0.0 {
            rows.push(row("Armour", format!("stops {:.0}% of each blow", c.armor * 100.0)));
        }
        if c.aim != 0.0 {
            rows.push(row("Aim", format!("{:+.0}% chance to hit", c.aim * 100.0)));
        }
        if c.speed != 1.0 {
            rows.push(row("Speed", format!("{:+.0}% walking, and in battle per action point", (c.speed - 1.0) * 100.0)));
        }
        rows.push(row("Installed", "by a doctor at an operating table (in place of a lost part, or electively)"));
        s.section("Cyberware", rows);
    }

    if let Some(u) = &d.utility {
        let effect = match u.effect {
            UtilityEffect::Grenade { damage, range } => {
                format!("thrown up to {:.0} cells at a unit: {:.0}-{:.0} damage to everyone in it, whatever their cover", range, damage.0, damage.1)
            }
            UtilityEffect::Medkit { quality } => format!("tends everyone's wounds in the unit ({:.0}% quality, more with Medicine skill)", quality * 100.0),
            UtilityEffect::Shield { energy, recharge } => format!(
                "takes up to {:.0} damage from shots and blasts (not blows), {:.0} back each round; full again after the battle",
                energy, recharge
            ),
        };
        let mut rows = vec![row("In battle", effect), row("Carried", format!("up to {} on the belt (the Utility slot)", u.carry))];
        if matches!(u.effect, UtilityEffect::Shield { .. }) {
            rows.push(row("But", "its field stops the wearer's own shots: no ranged weapons, so they fight (and squad) in melee"));
        } else {
            rows.push(row("Costs", format!("{} AP to use, and one is used up", u.ap)));
        }
        s.section("Belt item", rows);
    }

    let mut made = Vec::new();
    for r in RecipeId::all() {
        let rd = r.def();
        if rd.products.iter().any(|a| a.item == k) {
            let bench = rd.bench.id();
            let how = format!("{} -> {}, {:.0} s ({})", amounts(&rd.ingredients), amounts(&rd.products), rd.time, rd.skill.name());
            made.push(row(&format!("{} at {}", rd.name, bench.name()), how));
            s.link(InfoTarget::Build(bench));
        }
    }
    s.section("Made by", made);

    let mut used = Vec::new();
    let recipes: Vec<String> = RecipeId::all()
        .filter(|r| r.def().ingredients.iter().any(|a| a.item == k))
        .map(|r| r.def().name.clone())
        .collect();
    if !recipes.is_empty() {
        used.push(row("Recipes", recipes.join(", ")));
    }
    let builds: Vec<BuildId> = BuildId::all().filter(|b| b.def().cost.iter().any(|a| a.item == k)).collect();
    if !builds.is_empty() {
        let names: Vec<&str> = builds.iter().map(|b| b.name()).collect();
        used.push(row("Building", names.join(", ")));
        for b in builds {
            s.link(InfoTarget::Build(b));
        }
    }
    s.section("Used for", used);

    for p in PlantId::all().filter(|p| p.def().harvest.yields.item == k) {
        s.link(InfoTarget::Plant(p));
    }
    s
}

/// What a buildable is for, in a sentence (nothing for most).
pub fn purpose(d: &BuildDef) -> Option<&'static str> {
    Some(match d.special {
        Special::Floor => "A floor slab for upper storeys: build it in the air, held up by walls, doors or ground within 3 cells",
        Special::Roof => "Put over enclosed rooms by themselves",
        Special::Stairs if d.climb => "Connects this level with the one above: climbed from its open side (stacked, one long ladder)",
        Special::Stairs => "Connects this level with the one above: walk on at the low end and off the high end, like a slope",
        Special::Bed if d.medical => "For patients and operations: nobody sleeps here",
        Special::Bed if d.comfort < 1.0 => "A place on the ground to sleep and recover: better than nothing, worse than a bed",
        Special::Bed => "Colonists sleep and recover here",
        Special::Door => "Lets colonists through but closes the room",
        Special::Table => "Colonists eat here, sitting on a chair beside it",
        Special::Seat => "Put it beside a table to eat at",
        Special::Grave => "Dug into soil; haulers bury a dead colonist in it",
        Special::None => return None,
    })
}

fn build_sheet(k: BuildId) -> Sheet {
    let d = k.def();
    let mut s = Sheet::new(&d.name, &d.category);
    s.about.extend(purpose(d).map(String::from));
    if d.on_wall {
        s.about.push("Hangs on a wall: in one of five places on either side, counting for the room it faces.".into());
        if d.id == "calendar" {
            s.about.push("Shows the day and time (click its wall).".into());
        }
    }
    if !d.recipes.is_empty() {
        s.about.push("A workbench: click it in the world to add bills.".into());
        if d.turnable() {
            s.about.push("Worked from its front (where the arrow points when placing), or from beside it if that's blocked.".into());
        }
    }

    let mut building = Vec::new();
    building.push(row("Cost", if d.free() { "free".to_string() } else { amounts(&d.cost) }));
    building.push(row("Work", format!("{:.0} s of {} ({})", d.work_time, d.work.name().to_lowercase(), d.skill.name())));
    let back: Vec<String> = d.drops().iter().map(|(item, n)| format!("{} {}", n, item.name().to_lowercase())).collect();
    building.push(row("Taken down", if back.is_empty() { "gives nothing back".into() } else { format!("gives back {}", back.join(", ")) }));
    s.section("Building", building);
    for a in &d.cost {
        s.link(InfoTarget::Item(a.item));
    }

    let mut props = Vec::new();
    let walk = if d.special == Special::Stairs {
        "climbed from the low end"
    } else if d.passable {
        "yes"
    } else {
        "no, it blocks the way"
    };
    if d.on_wall {
        props.push(row("Placement", "on a wall, one of five places per side"));
    } else {
        props.push(row("Walk through", walk));
        props.push(row("Cover in battle", format!("{:?}", d.cover_kind())));
    }
    if d.blocks_room && d.special != Special::Door {
        props.push(row("Rooms", "encloses them"));
    }
    if d.holds_floors {
        props.push(row("Holds up", "floors and roofs within 3 cells, and walls on top"));
    }
    if d.daylight {
        props.push(row("Daylight", "lets it into the room"));
    }
    if d.open_sky {
        props.push(row("Placement", "needs open sky, not under a roof"));
    }
    if d.special == Special::Bed {
        props.push(row("Comfort", format!("{:.0}%", d.comfort * 100.0)));
    }
    if d.beauty != 0.0 {
        props.push(row("Beauty", format!("{:+}", d.beauty)));
    }
    if let Some(l) = &d.light {
        props.push(row("Light", format!("lights up {:.0} cells around", l.range)));
    }
    let electric = d.power.is_some_and(|p| p.draw > 0.0);
    if d.fun > 0.0 {
        props.push(row("Fun", format!("colonists have fun at it, {:.1}× as fast as on a stroll", d.fun / 0.7)));
    }
    if d.heat != 0.0 {
        let (what, when) = match (d.heat > 0.0, electric) {
            (true, false) => ("warms", String::new()),
            (true, true) => ("warms", format!(", while it runs: when the room is below {:.0}°C", my_world::power::HEAT_BELOW)),
            (false, _) => ("cools", format!(", while it runs: when the room is above {:.0}°C", my_world::power::COOL_ABOVE)),
        };
        props.push(row("Heat", format!("{} its room by about {:.0}°C (less in a big room){}", what, d.heat.abs(), when)));
    }
    if let Some(st) = &d.storage {
        props.push(row("Storage", format!("{} stacks of up to {} (different things)", st.slots, STACK_MAX)));
    }
    if let Some(power) = d.power_text() {
        props.push(row("Power", format!("{} (one grid for the whole colony)", power)));
    }
    s.section("Properties", props);

    let mut bench = Vec::new();
    for r in &d.recipes {
        let rd = r.def();
        let how = format!("{} -> {}, {:.0} s ({})", amounts(&rd.ingredients), amounts(&rd.products), rd.time, rd.skill.name());
        bench.push(row(&rd.name, how));
        for a in &rd.products {
            s.link(InfoTarget::Item(a.item));
        }
    }
    s.section("Makes", bench);

    let rooms: Vec<&str> = defs()
        .rooms
        .iter()
        .filter(|room| room.needs.iter().any(|n| n.includes(k)))
        .map(|room| room.name.as_str())
        .collect();
    if !rooms.is_empty() {
        s.section("Rooms", vec![row("Counts towards", rooms.join(", "))]);
    }
    s
}

fn plant_sheet(k: PlantId) -> Sheet {
    let d = k.def();
    let mut s = Sheet::new(&d.name, "Plant");
    let mut growing = Vec::new();
    if d.grow_time > 0.0 {
        let light_per_day = (DUSK - DAWN) / 24.0 * DAY_LEN;
        let days = d.grow_time / light_per_day;
        growing.push(row("Grows", format!("in {:.1} days, only in daylight and under open sky", days)));
    }
    if let Some(sow) = &d.sow {
        growing.push(row("Sown", format!("in grow zones, {:.0} s of {} ({})", sow.time, sow.work.name().to_lowercase(), sow.skill.name())));
        growing.push(row("Seasons", "grows in spring and summer, slower in autumn, not in winter; the first frost kills it unripe"));
    }
    if d.wild.is_some() {
        growing.push(row("Wild", "grows by itself on open soil"));
    }
    s.section("Growing", growing);

    let h = &d.harvest;
    let mut harvest = vec![
        row("Yields", format!("{} {}", h.yields.count, h.yields.item.name().to_lowercase())),
        row("Work", format!("{:.0} s of {} ({})", h.time, h.work.name().to_lowercase(), h.skill.name())),
    ];
    harvest.push(row("When", if h.desig.is_some() { "once you mark it" } else { "by itself once grown" }));
    s.section("Harvest", harvest);
    s.link(InfoTarget::Item(h.yields.item));

    let walk = if d.passable { "yes" } else { "no, it blocks the way" };
    s.section("Properties", vec![row("Walk through", walk), row("Cover in battle", format!("{:?}", d.cover_kind()))]);
    s
}

/// The info window, while something is picked (`UiState::info`) or the
/// lookup is open.
pub fn window(ctx: &egui::Context, st: &mut UiState) {
    if !st.show_info {
        return;
    }
    let mut open = true;
    let mut go: Option<InfoTarget> = None;
    theme::FieldWindow::new("Info")
        .id(egui::Id::new("info_window"))
        .open(&mut open)
        .anchor(Align2::RIGHT_TOP, [-8.0, 48.0])
        .default_width(380.0)
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!st.info_back.is_empty(), egui::Button::new("<")).on_hover_text("Back").clicked() {
                    st.info = st.info_back.pop();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut st.info_filter)
                        .hint_text("Look up anything...")
                        .desired_width(f32::INFINITY),
                );
            });
            let query = st.info_filter.trim().to_lowercase();
            if !query.is_empty() {
                egui::ScrollArea::vertical().id_salt("info_lookup").max_height(160.0).show(ui, |ui| {
                    let found: Vec<InfoTarget> = InfoTarget::all().filter(|t| t.name().to_lowercase().contains(&query)).collect();
                    if found.is_empty() {
                        ui.label(RichText::new("Nothing matches").weak());
                    }
                    for t in found {
                        if ui.selectable_label(st.info == Some(t), t.name()).clicked() {
                            go = Some(t);
                        }
                    }
                });
            }
            ui.separator();
            let Some(t) = st.info else {
                ui.label(RichText::new("Search above, or click Info on anything in the inspector.").weak());
                return;
            };
            let sh = sheet(t);
            ui.horizontal(|ui| {
                ui.label(theme::caps(&sh.title, 22.0).color(theme::CREAM_BRIGHT));
                ui.label(RichText::new(&sh.kind).weak());
            });
            for line in &sh.about {
                ui.label(line);
            }
            egui::ScrollArea::vertical().id_salt("info_sheet").max_height(460.0).show(ui, |ui| {
                for sec in &sh.sections {
                    ui.add_space(4.0);
                    theme::section(ui, sec.title);
                    egui::Grid::new(("info", sec.title)).num_columns(2).striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                        for (label, value) in &sec.rows {
                            ui.label(RichText::new(label).weak());
                            ui.add(egui::Label::new(value).wrap());
                            ui.end_row();
                        }
                    });
                }
                if !sh.links.is_empty() {
                    ui.add_space(4.0);
                    theme::section(ui, "See also");
                    ui.horizontal_wrapped(|ui| {
                        for &l in &sh.links {
                            if ui.small_button(l.name()).clicked() {
                                go = Some(l);
                            }
                        }
                    });
                }
            });
        });
    if let Some(t) = go {
        st.open_info(t);
        st.info_filter.clear();
    }
    if !open {
        st.show_info = false;
    }
}

/// A small "Info" button that opens the sheet for `t`.
pub fn button(ui: &mut egui::Ui, t: InfoTarget, to: &mut Option<InfoTarget>) {
    if ui.small_button("Info").on_hover_text(format!("All about {}", t.name().to_lowercase())).clicked() {
        *to = Some(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_thing_has_a_sheet() {
        for t in InfoTarget::all() {
            let s = sheet(t);
            assert!(!s.sections.is_empty(), "{:?} says nothing", t);
            assert!(!s.links.contains(&t), "{:?} links to itself", t);
        }
    }

    #[test]
    fn weapons_show_their_stats() {
        let rifle = ItemId::all().find(|k| k.def().weapon.as_ref().is_some_and(|w| w.ranged())).unwrap();
        let s = sheet(InfoTarget::Item(rifle));
        let w = rifle.def().weapon.as_ref().unwrap();
        assert_eq!(s.row("Type"), Some("Ranged"));
        assert_eq!(s.row("Range"), Some(format!("{:.0} cells", w.range).as_str()));
        assert!(s.row("Damage").unwrap().starts_with(&format!("{:.0}-{:.0}", w.damage.0, w.damage.1)));
        assert!(s.row("Damage per second").is_some());
    }

    #[test]
    fn workbenches_list_what_they_make() {
        let bench = BuildId::all().find(|k| !k.def().recipes.is_empty()).unwrap();
        let s = sheet(InfoTarget::Build(bench));
        let makes = s.sections.iter().find(|sec| sec.title == "Makes").expect("a Makes section");
        assert_eq!(makes.rows.len(), bench.def().recipes.len());
        assert!(s.row("Cost").is_some() && s.row("Cover in battle").is_some());
        // And the things it makes point back at it.
        let product = bench.def().recipes[0].def().products[0].item;
        assert!(sheet(InfoTarget::Item(product)).links.contains(&InfoTarget::Build(bench)));
    }

    #[test]
    fn wall_decor_says_where_it_goes() {
        let lamp = sheet(InfoTarget::Build(BuildId::named("wall_lamp")));
        assert!(lamp.row("Placement").is_some_and(|p| p.contains("wall")));
        assert!(lamp.row("Light").is_some() && lamp.row("Walk through").is_none());
        assert!(sheet(InfoTarget::Build(BuildId::named("wall_shelf"))).row("Storage").is_some());
    }

    #[test]
    fn crops_tell_how_many_days_they_take() {
        let crop = PlantId::all().find(|k| k.def().sow.is_some()).unwrap();
        let s = sheet(InfoTarget::Plant(crop));
        assert!(s.row("Grows").unwrap().contains("days"), "{:?}", s.row("Grows"));
        assert!(s.links.contains(&InfoTarget::Item(crop.def().harvest.yields.item)));
    }

    #[test]
    fn food_says_how_long_it_lasts() {
        let meal = ItemId::all().find(|k| k.def().nutrition > 0.0 && !k.def().raw).unwrap();
        assert!(sheet(InfoTarget::Item(meal)).row("Nutrition").unwrap().contains("lasts about"));
    }
}
