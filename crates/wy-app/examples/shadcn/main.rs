mod dashboard;
mod state;
mod tokens;
mod widgets;

use std::rc::Rc;

use wy_engine::mve_integration::MveApp;
use wy_mve::NodeContext;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = state::App::new();
    let app = Rc::new(app);

    let app_handle = Rc::clone(&app);
    let mve = MveApp::new(move |cx: &mut NodeContext| {
        let node = dashboard::dashboard(&app_handle);
        cx.child(node);
    });

    wy_engine::runner::run(mve)
}
