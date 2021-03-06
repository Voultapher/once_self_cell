use self_cell::self_cell;

struct Dependent<'a> {
    good: &'a String,
    bad: &'a String,
}

self_cell!(
    struct NoCov {
        owner: String,

        #[covariant]
        dependent: Dependent,
    }
);

fn main() {
    let outside_string = String::from("outside string");

    let _cell = NoCov::new("hi this is no good".into(), |owner| Dependent {
        good: owner,
        bad: &outside_string,
    });
}
