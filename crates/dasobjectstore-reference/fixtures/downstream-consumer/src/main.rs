use dasobjectstore_reference::{EvidenceRefV1, ObjectRefV1};

fn main() {
    ObjectRefV1::decode(include_bytes!("../../object-ref-v1.json"))
        .expect("the packaged positive ObjectRefV1 vector decodes");
    ObjectRefV1::decode(include_bytes!("../../object-ref-v1-max-safe-integer.json"))
        .expect("the packaged max-safe-integer ObjectRefV1 vector decodes");
    EvidenceRefV1::decode(include_bytes!("../../evidence-ref-v1.json"))
        .expect("the packaged positive EvidenceRefV1 vector decodes");
}
