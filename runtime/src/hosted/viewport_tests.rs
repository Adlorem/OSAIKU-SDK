use super::*;

struct App {
    size: (u32, u32),
}
impl WindowApplication for App {
    fn new() -> Self {
        Self { size: (0, 0) }
    }
    fn event(&mut self, _: &osaiku_ui::input::Event) -> bool {
        false
    }
    fn render(&mut self, _: &mut [u8]) -> Result<usize, osaiku_ui::Error> {
        Ok(7)
    }
    fn render_for_viewport(
        &mut self,
        w: u32,
        h: u32,
        bytes: &mut [u8],
    ) -> Result<usize, osaiku_ui::Error> {
        let _ = bytes;
        self.size = (w, h);
        Ok(9)
    }
    fn prepare_shutdown(&mut self) -> bool {
        true
    }
}

#[test]
fn offered_viewport_excludes_title_and_tracks_each_frame_resize() {
    let mut app = App::new();
    let mut bytes = [0; 16];
    for (w, h, title) in [(640, 480, 32), (1000, 700, 40), (800, 600, 0)] {
        let mut offer = crate::surface::FrameOffer::default();
        offer.geometry.window_width = w;
        offer.geometry.window_height = h;
        offer.geometry.content = osaiku_ui::surface::Rect {
            x: 0,
            y: title,
            width: w,
            height: h - title,
        };
        assert_eq!(render_offered(&mut app, &offer, &mut bytes), Ok(9));
        assert_eq!(app.size, (w, h - title));
    }
}

struct Legacy;
impl WindowApplication for Legacy {
    fn new() -> Self {
        Self
    }
    fn event(&mut self, _: &osaiku_ui::input::Event) -> bool {
        false
    }
    fn render(&mut self, _: &mut [u8]) -> Result<usize, osaiku_ui::Error> {
        Ok(7)
    }
    fn prepare_shutdown(&mut self) -> bool {
        true
    }
}

#[test]
fn existing_apps_can_keep_their_recipe_rendering() {
    let mut app = Legacy::new();
    assert_eq!(
        render_offered(
            &mut app,
            &crate::surface::FrameOffer::default(),
            &mut [0; 16]
        ),
        Ok(7)
    );
}
