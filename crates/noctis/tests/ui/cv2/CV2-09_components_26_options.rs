use noctis::prelude::*;

#[derive(StringSelectMenu)]
#[string_select("t.pick", defaults(placeholder = "Pick"))]
struct Pick;

impl Pick {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = components! { row [ Pick.build().options([noctis::ui::SelectOption::new("0", "0"), noctis::ui::SelectOption::new("1", "1"), noctis::ui::SelectOption::new("2", "2"), noctis::ui::SelectOption::new("3", "3"), noctis::ui::SelectOption::new("4", "4"), noctis::ui::SelectOption::new("5", "5"), noctis::ui::SelectOption::new("6", "6"), noctis::ui::SelectOption::new("7", "7"), noctis::ui::SelectOption::new("8", "8"), noctis::ui::SelectOption::new("9", "9"), noctis::ui::SelectOption::new("10", "10"), noctis::ui::SelectOption::new("11", "11"), noctis::ui::SelectOption::new("12", "12"), noctis::ui::SelectOption::new("13", "13"), noctis::ui::SelectOption::new("14", "14"), noctis::ui::SelectOption::new("15", "15"), noctis::ui::SelectOption::new("16", "16"), noctis::ui::SelectOption::new("17", "17"), noctis::ui::SelectOption::new("18", "18"), noctis::ui::SelectOption::new("19", "19"), noctis::ui::SelectOption::new("20", "20"), noctis::ui::SelectOption::new("21", "21"), noctis::ui::SelectOption::new("22", "22"), noctis::ui::SelectOption::new("23", "23"), noctis::ui::SelectOption::new("24", "24"), noctis::ui::SelectOption::new("25", "25")]) ] };
}
