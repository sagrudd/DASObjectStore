#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use prosopikon_core::{mnemosyne_role_templates, DomainRoleTemplate};
use prosopikon_yew::{LocalAccessUserEditor, LocalAccessUserEditorDraft};
use wasm_bindgen_test::*;
use web_sys::{Event, HtmlButtonElement, HtmlSelectElement};
use yew::prelude::*;

wasm_bindgen_test_configure!(run_in_browser);

// These props and callbacks use DAS's root-resolved Yew and Core types. Passing
// them into the supplier's component checks the actual downstream nominal boundary.
#[derive(Clone, Properties, PartialEq)]
struct ConsumerProps {
    draft: LocalAccessUserEditorDraft,
    roles: Vec<DomainRoleTemplate>,
    enabled: bool,
    changes: Rc<RefCell<Vec<LocalAccessUserEditorDraft>>>,
    submissions: Rc<RefCell<Vec<LocalAccessUserEditorDraft>>>,
}

#[function_component]
fn DownstreamConsumer(props: &ConsumerProps) -> Html {
    let changes = props.changes.clone();
    let submissions = props.submissions.clone();
    let on_change: Callback<LocalAccessUserEditorDraft> =
        Callback::from(move |draft| changes.borrow_mut().push(draft));
    let on_submit: Callback<LocalAccessUserEditorDraft> =
        Callback::from(move |draft| submissions.borrow_mut().push(draft));
    html! {
        <LocalAccessUserEditor
            draft={props.draft.clone()}
            role_templates={props.roles.clone()}
            enabled={props.enabled}
            {on_change}
            {on_submit}
        />
    }
}

async fn settle() {
    TimeoutFuture::new(0).await;
}

#[wasm_bindgen_test]
async fn root_typed_role_callbacks_and_disabled_submission_cross_supplier_boundary() {
    use web_sys::wasm_bindgen::JsCast;

    let document = web_sys::window().unwrap().document().unwrap();
    let root = document.create_element("div").unwrap();
    document.body().unwrap().append_child(&root).unwrap();
    let roles: Vec<DomainRoleTemplate> = mnemosyne_role_templates();
    assert!(roles.iter().any(|role| role.code == "scientist"));
    assert!(roles.iter().any(|role| role.code == "sponsor"));
    let changes = Rc::new(RefCell::new(Vec::new()));
    let submissions = Rc::new(RefCell::new(Vec::new()));
    let initial = LocalAccessUserEditorDraft {
        username: "synthetic-das-consumer".into(),
        role_code: "scientist".into(),
        ..Default::default()
    };
    let mut props = ConsumerProps {
        draft: initial.clone(),
        roles,
        enabled: true,
        changes: changes.clone(),
        submissions: submissions.clone(),
    };
    let mut handle =
        yew::Renderer::<DownstreamConsumer>::with_root_and_props(root.clone(), props.clone())
            .render();
    settle().await;
    let select = || {
        root.query_selector("select")
            .unwrap()
            .unwrap()
            .dyn_into::<HtmlSelectElement>()
            .unwrap()
    };
    let submit = || {
        root.query_selector("button")
            .unwrap()
            .unwrap()
            .dyn_into::<HtmlButtonElement>()
            .unwrap()
    };
    assert_eq!(select().value(), "scientist");
    assert!(!select().disabled());
    assert!(!submit().disabled());

    select().set_value("sponsor");
    select()
        .dispatch_event(&Event::new("change").unwrap())
        .unwrap();
    settle().await;
    let mut sponsor = initial.clone();
    sponsor.role_code = "sponsor".into();
    assert_eq!(*changes.borrow(), vec![sponsor.clone()]);
    assert!(submissions.borrow().is_empty());
    props.draft = sponsor.clone();
    handle.update(props.clone());
    settle().await;
    assert_eq!(select().value(), "sponsor");
    submit().click();
    settle().await;
    assert_eq!(*submissions.borrow(), vec![sponsor.clone()]);

    props.enabled = false;
    handle.update(props.clone());
    settle().await;
    assert!(select().disabled());
    assert!(submit().disabled());
    submit().click();
    settle().await;
    assert_eq!(*submissions.borrow(), vec![sponsor.clone()]);

    props.enabled = true;
    props.draft.username.clear();
    handle.update(props.clone());
    settle().await;
    assert!(!select().disabled());
    assert!(submit().disabled());
    submit().click();
    settle().await;
    assert_eq!(*submissions.borrow(), vec![sponsor.clone()]);
    handle.destroy();
    settle().await;
    assert!(root.query_selector("select").unwrap().is_none());

    props.draft = initial.clone();
    let handle =
        yew::Renderer::<DownstreamConsumer>::with_root_and_props(root.clone(), props).render();
    settle().await;
    assert_eq!(select().value(), "scientist");
    submit().click();
    settle().await;
    assert_eq!(*submissions.borrow(), vec![sponsor, initial]);
    assert_eq!(changes.borrow().len(), 1);
    handle.destroy();
    settle().await;
    root.remove();
}
