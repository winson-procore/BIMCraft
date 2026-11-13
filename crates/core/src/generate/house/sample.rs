//! The graph of a house, sampled by the seed: how many rooms of each kind, which have a closet or an en-suite bath, how the
//! shared rooms are joined and where the way in and the porch are. Every choice is made inside what the house rule pack
//! allows (`data/graph-rules/house.json`), so the graph passes its checker before any geometry exists for it.

use super::super::Rng;
use super::{Bed, HouseGraph, Kind, Link, Means, Room};

const SALT: u64 = 0x4A05_E001;

fn push(rooms: &mut Vec<Room>, id: String, name: String, kind: Kind) -> usize {
    rooms.push(Room { id, name, kind });
    rooms.len() - 1
}

/// The graph of a house of about `area_m2` square metres.
pub fn sample(seed: u64, area_m2: f64) -> HouseGraph {
    let mut rng = Rng::new(seed, SALT);
    let mut chance = |percent: u64| rng.below(100) < percent;
    let bedrooms: usize = match area_m2 {
        a if a < 78.0 => 1,
        a if a < 100.0 => 1 + usize::from(chance(60)),
        a if a < 125.0 => 2,
        a if a < 150.0 => 2 + usize::from(chance(60)),
        _ => 3,
    };
    let dining = area_m2 >= 100.0 && chance(if area_m2 >= 125.0 { 90 } else { 45 });
    let porch = area_m2 >= 55.0 && chance(60);
    let cap_kind = if chance(45) { Some(if chance(50) { Kind::Closet } else { Kind::Laundry }) } else { None };
    let hall_open = chance(40);
    let first_open = chance(50);

    let mut rooms = Vec::new();
    let mut public: Vec<usize> = Vec::new();
    let mut order = vec![Kind::Living, Kind::Kitchen];
    if dining {
        order.push(Kind::Dining);
    }
    let mut rng = Rng::new(seed, SALT + 1);
    rng.shuffle(&mut order);
    for kind in order {
        public.push(push(&mut rooms, kind.name().into(), kind.label().into(), kind));
    }
    let hall = push(&mut rooms, "hall".into(), "Hall".into(), Kind::Hall);

    let mut rng = Rng::new(seed, SALT + 2);
    let mut beds = Vec::new();
    for k in 1..=bedrooms {
        let room = push(&mut rooms, format!("bedroom_{k}"), format!("Bedroom {k}"), Kind::Bedroom);
        beds.push(Bed { room, closet: None, bath: None });
    }
    let hall_bath = push(&mut rooms, "bath".into(), "Bath".into(), Kind::Bath);
    let mut links = Vec::new();
    for (k, bed) in beds.iter_mut().enumerate() {
        let k = k + 1;
        if rng.below(100) < 72 {
            bed.closet = Some(push(&mut rooms, format!("closet_{k}"), format!("Closet {k}"), Kind::Closet));
        }
        if rng.below(100) < 30 {
            bed.bath = Some(push(&mut rooms, format!("ensuite_{k}"), format!("Ensuite {k}"), Kind::Bath));
        }
    }
    let sliding_closets: Vec<bool> = beds.iter().map(|_| rng.below(100) < 60).collect();
    let cap = cap_kind.map(|kind| push(&mut rooms, kind.name().into(), kind.label().into(), kind));
    let mut rng = Rng::new(seed, SALT + 3);
    let porch_off = public[rng.below(public.len() as u64) as usize];
    let porch = porch.then(|| push(&mut rooms, "porch".into(), "Porch".into(), Kind::Porch));
    let hall_to = public[rng.below(public.len() as u64) as usize];
    let porch_sliding = rng.below(100) < 50;

    // The shared rooms lie in a row; neighbours are joined, and a dining room only openly.
    let mut opened = false;
    for pair in public.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let needs_open = rooms[a].kind == Kind::Dining || rooms[b].kind == Kind::Dining;
        let open = needs_open || (first_open && !opened);
        opened |= open;
        links.push(Link { a, b, means: if open { Means::Open } else { Means::Swing } });
    }
    // A porch is closed off by room separation lines, which a house with no open edge has none of.
    if porch.is_some()
        && !opened
        && let Some(link) = links.first_mut()
    {
        link.means = Means::Open;
    }
    links.push(Link { a: hall, b: hall_to, means: if hall_open { Means::Open } else { Means::Swing } });
    for bed in &beds {
        links.push(Link { a: hall, b: bed.room, means: Means::Swing });
    }
    links.push(Link { a: hall, b: hall_bath, means: Means::Swing });
    for (bed, sliding) in beds.iter().zip(sliding_closets) {
        if let Some(c) = bed.closet {
            links.push(Link { a: bed.room, b: c, means: if sliding { Means::Sliding } else { Means::Swing } });
        }
        if let Some(b) = bed.bath {
            links.push(Link { a: bed.room, b, means: Means::Swing });
        }
    }
    if let Some(c) = cap {
        links.push(Link { a: hall, b: c, means: Means::Swing });
    }
    if let Some(p) = porch {
        links.push(Link { a: porch_off, b: p, means: if porch_sliding { Means::Sliding } else { Means::Swing } });
    }
    // The way in: a living room or a kitchen, never the room the porch is off.
    let living = rooms.iter().position(|r| r.kind == Kind::Living).unwrap_or(0);
    let kitchen = rooms.iter().position(|r| r.kind == Kind::Kitchen).unwrap_or(0);
    let entry = match (porch.is_some(), porch_off == living, porch_off == kitchen) {
        (true, true, _) => kitchen,
        (true, _, true) => living,
        _ => {
            if rng.below(100) < 60 {
                living
            } else {
                kitchen
            }
        }
    };
    HouseGraph { rooms, links, entry, public, hall, hall_to, beds, hall_bath, cap, porch, porch_off }
}
