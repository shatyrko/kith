//! Writes a small address book of invented people: `cargo run -p kith-core --example demo -- <folder>`
//! (for screenshots and trying the app: `KITH_DATA_DIR=<folder> kith`).

use kith_core::store::Store;
use kith_core::*;

fn l(label: &str, value: &str) -> Labeled {
    Labeled::new(label, value)
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: demo <folder>");
    let mut store = Store::open(&dir).expect("open");

    let mut a = Contact::new();
    a.name = Name { prefix: "Dr.".into(), given: "Amelia".into(), middle: "Rose".into(), family: "Hart".into(), suffix: "PhD".into(), maiden: "Whitfield".into(), ..Default::default() };
    a.nicknames = vec!["Mel".into()];
    a.favorite = true;
    a.pronouns = "she/her".into();
    a.languages = vec!["English".into(), "French".into()];
    a.timezone = "Europe/London".into();
    a.orgs = vec![Org { company: "Northwind Labs".into(), department: "Research".into(), title: "Head of Design".into(), phonetic: String::new() }];
    a.phones = vec![Labeled { pref: true, ..l("mobile", "+44 7700 900123") }, l("work", "+44 20 7946 0958")];
    a.emails = vec![Labeled { pref: true, ..l("work", "amelia.hart@northwind.example") }, l("home", "mel@hart.example")];
    a.addresses = vec![Address { label: "home".into(), extended: "Flat 4".into(), street: "12 Brook Lane".into(), city: "Bristol".into(), region: "Avon".into(), postcode: "BS1 4DJ".into(), country: "United Kingdom".into(), ..Default::default() }];
    a.urls = vec![l("homepage", "https://amelia.example")];
    a.social = vec![l("mastodon", "@mel@social.example")];
    a.ims = vec![l("Signal", "@mel.42")];
    a.dates = vec![
        LabeledDate { label: "birthday".into(), year: Some(1988), month: 11, day: 3 },
        LabeledDate { label: "anniversary".into(), year: Some(2015), month: 6, day: 20 },
    ];
    a.relations = vec![l("spouse", "Jonas Hart"), l("assistant", "Priya Nair")];
    a.categories = vec!["Work".into(), "Friends".into()];
    a.notes = "Met at the design conference in Lisbon.\nPrefers calls before noon.".into();
    a.interests = vec!["Climbing".into(), "Letterpress".into()];
    a.custom = vec![l("Allergies", "Peanuts")];
    store.save(a).unwrap();

    let first = ["Noah", "Sofia", "Liam", "Mira", "Ethan", "Chloe", "Oscar", "Ivy", "Leo", "Nora", "Felix", "Zoe", "Hugo", "Elena", "Marco", "Aiko", "Kenji", "Priya", "Arjun", "Layla", "Omar", "Greta", "Lars", "Tomás", "Inés", "Dmitri", "Katya", "Yusuf", "Amara", "Jonas"];
    let last = ["Lindqvist", "Moreau", "Okafor", "Rossi", "Tanaka", "Novak", "Silva", "Brandt", "Kowalski", "Nair", "Haddad", "Fischer", "Bianchi", "Sørensen", "Ibarra", "Volkov", "Mensah", "Park", "Dubois", "Hart"];
    let cos = ["Northwind Labs", "Helio Studio", "Bluebird Café", "Orbit Logistics", "Pinecone Bank", "Atlas Design", "Quill & Co.", "Lumen Health"];
    let jobs = ["Product Designer", "Engineer", "Accountant", "Barista", "Architect", "Physician", "Photographer", "Teacher"];
    for i in 0..36usize {
        let (f, s) = (first[i % first.len()], last[(i * 7 + 3) % last.len()]);
        let mut c = Contact::new();
        c.name.given = f.into();
        c.name.family = s.into();
        c.phones = vec![l(if i % 3 == 0 { "home" } else { "mobile" }, &format!("+1 555 {:03} {:04}", 100 + (i * 37) % 900, 1000 + (i * 193) % 9000))];
        if i % 4 != 3 {
            let mail: String = format!("{}.{}@example.org", f, s).to_lowercase().chars().filter(|c| c.is_ascii()).collect();
            c.emails = vec![l("home", &mail)];
        }
        if i % 2 == 1 {
            c.orgs = vec![Org { company: cos[i % cos.len()].into(), title: jobs[i % jobs.len()].into(), ..Default::default() }];
        }
        c.categories = match i % 5 {
            0 => vec!["Family".into()],
            1 => vec!["Friends".into()],
            2 => vec!["Work".into()],
            _ => vec![],
        };
        c.favorite = i % 9 == 0;
        store.save(c).unwrap();
    }

    let mut cafe = Contact::new();
    cafe.kind = "org".into();
    cafe.orgs = vec![Org { company: "Bluebird Café".into(), ..Default::default() }];
    cafe.phones = vec![l("main", "+1 415 555 0142")];
    cafe.urls = vec![l("homepage", "https://bluebird.example")];
    cafe.addresses = vec![Address { label: "work".into(), street: "88 Market St".into(), city: "San Francisco".into(), region: "CA".into(), postcode: "94103".into(), country: "USA".into(), ..Default::default() }];
    cafe.categories = vec!["Places".into()];
    store.save(cafe).unwrap();
    println!("{} invented contacts in {}", store.len(), dir);
}
