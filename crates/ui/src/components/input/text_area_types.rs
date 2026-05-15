pub struct TextArea {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_lines: Vec<WrappedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_visual_line_count: usize,
    scroll_handle: ScrollHandle,
    is_selecting: bool,
    height_rem: f32,
    padding_x_rem: f32,
    padding_top_rem: f32,
    padding_bottom_rem: f32,
    framed: bool,
    pending_reveal_cursor: bool,
}

impl TextArea {
    pub fn new(
        initial: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let content: SharedString = initial.into();
        let len = content.len();
        Self {
            focus_handle: cx.focus_handle(),
            content,
            placeholder: placeholder.into(),
            selected_range: len..len,
            selection_reversed: false,
            marked_range: None,
            last_lines: Vec::new(),
            last_bounds: None,
            last_visual_line_count: 3,
            scroll_handle: ScrollHandle::new(),
            is_selecting: false,
            height_rem: 4.75,
            padding_x_rem: 0.5,
            padding_top_rem: 0.25,
            padding_bottom_rem: 0.25,
            framed: true,
            pending_reveal_cursor: false,
        }
    }

    pub fn height_rem(mut self, height_rem: f32) -> Self {
        self.height_rem = height_rem;
        self
    }

    pub fn padding_bottom_rem(mut self, padding_bottom_rem: f32) -> Self {
        self.padding_bottom_rem = padding_bottom_rem;
        self
    }

    pub fn padding_x_rem(mut self, padding_x_rem: f32) -> Self {
        self.padding_x_rem = padding_x_rem;
        self
    }

    pub fn padding_top_rem(mut self, padding_top_rem: f32) -> Self {
        self.padding_top_rem = padding_top_rem;
        self
    }

    pub fn unframed(mut self) -> Self {
        self.framed = false;
        self
    }

    pub fn value(&self) -> &SharedString {
        &self.content
    }

    pub fn set_value(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        let value: SharedString = value.into();
        if self.content == value {
            return;
        }
        self.content = value;
        let len = self.content.len();
        self.selected_range = len..len;
        self.marked_range = None;
        self.pending_reveal_cursor = true;
        cx.notify();
    }

    fn emit_changed(&self, cx: &mut Context<Self>) {
        cx.emit(ValueChanged {
            value: self.content.clone(),
        });
    }

}
