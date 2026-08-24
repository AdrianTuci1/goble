mod hot_ui;
mod root_view;

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::AppContext;
use goble_ui::platform::run_with_root;

use crate::root_view::RootView;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let app_context = Rc::new(RefCell::new(AppContext::default()));
    let root = {
        let ctx = app_context.borrow();
        RootView::new(&ctx)
    };

    run_with_root(Box::new(root), app_context)
}
