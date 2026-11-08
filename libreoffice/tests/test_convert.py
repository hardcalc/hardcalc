"""Tests of the UNO-free part of the extension: python3 -m unittest discover libreoffice/tests"""

import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "extension", "pythonpath"))

from hardcalc_lo import convert, detect, markdown, styles  # noqa: E402
from hardcalc_lo.formula import FormulaError  # noqa: E402
from hardcalc_lo.translate import Book, TableInfo, from_calc, to_calc  # noqa: E402

BUDGET = os.path.join(HERE, "..", "..", "examples", "budget.hcmd")


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def calc(plans, state):
    """What Calc holds after the import (formulas as the filter wrote them)."""
    sheets = []
    for k, plan in enumerate(plans):
        values = [list(plan.header)]
        formulas = {}
        kinds = {}
        for r, row in enumerate(plan.cells, start=1):
            out = []
            for c, cell in enumerate(row):
                if cell is None:
                    out.append("")
                elif cell[0] == "f":
                    out.append(0.0)
                    formulas[(r, c)] = cell[1]
                    state["sheets"][k]["formulas"]["%d,%d" % (r, c)] = [cell[1], plan.formulas["%d,%d" % (r, c)]]
                elif cell[0] == "d":
                    out.append(cell[1])
                    kinds[(r, c)] = "date"
                else:
                    out.append(cell[1])
            values.append(out)
        grid = [[styles.DEFAULT] * len(plan.header) for _ in values]
        for r0, r1, c0, c1, parts in plan.format_ops:
            for r in range(r0 + 1, r1 + 2):
                for c in range(c0, c1 + 1):
                    for p in parts:
                        grid[r][c] = styles.apply_part(grid[r][c], p)
        sheets.append(convert.SheetData(plan.sheet, values, formulas, kinds, grid))
    return sheets


def set_style(sheet, r, c, **changes):
    """Formats cell (r, c) of SheetData as a user would in Calc."""
    st = list(sheet.styles[r][c])
    for name, value in changes.items():
        st[styles.FAMILIES.index(name)] = value
    sheet.styles[r][c] = tuple(st)


class Detection(unittest.TestCase):
    def test_by_name(self):
        for url in ("file:///C:/a/Budget.hcmd", "file:///a/b.HCMD", "file:///a/b.hcmd?x=1"):
            self.assertTrue(detect.by_name(url), url)
        for url in ("file:///a/README.md", "file:///a/b.hcmd.gz", "file:///a/hcmd", "", None):
            self.assertFalse(detect.by_name(url), url)

    def test_by_content(self):
        self.assertTrue(detect.by_content(read(BUDGET)))
        self.assertTrue(detect.by_content("# x\n\n```types\nEuro = decimal(2)\n```\n"))
        self.assertTrue(detect.by_content("## Cameras :: playground nofill\n"))
        self.assertFalse(detect.by_content("# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n"))
        self.assertFalse(detect.by_content("Text with a :: matrix in the middle of a sentence\n"))


class Numbers(unittest.TestCase):
    def test_parse_number(self):
        self.assertEqual(convert.parse_number("1200,00"), (1200.0, True, 2))
        self.assertEqual(convert.parse_number("1.200,50"), (1200.5, True, 2))
        self.assertEqual(convert.parse_number("1,200.50"), (1200.5, False, 2))
        self.assertEqual(convert.parse_number("42"), (42.0, False, 0))
        self.assertEqual(convert.parse_number("-0.5")[0], -0.5)
        self.assertIsNone(convert.parse_number("5 m/s"))
        self.assertIsNone(convert.parse_number("Nikon D70s"))
        self.assertIsNone(convert.parse_number("F FX"))

    def test_dates(self):
        self.assertEqual(convert.parse_date("2026-10-08"), 46303.0)
        self.assertEqual(convert.parse_date("08.10.2026"), 46303.0)
        self.assertIsNone(convert.parse_date("2026-02-30"))
        self.assertEqual(convert.serial_text(46303.0), "2026-10-08")
        self.assertEqual(convert.serial_text(46303.5), "2026-10-08 12:00")
        self.assertEqual(convert.serial_text(0.25), "06:00")

    def test_render(self):
        self.assertEqual(convert.render_number(1200.0, 2), "1200.00")
        self.assertEqual(convert.render_number(0.1 + 0.2), "0.3")
        self.assertEqual(convert.render_number(3.0), "3")
        # Files hold numbers with a decimal point.
        self.assertEqual(convert.point_text("1.200,50"), "1200.50")
        self.assertEqual(convert.point_text("1200.50"), "1200.50")
        self.assertEqual(convert.point_text("paid"), "paid")


class Markdown(unittest.TestCase):
    def test_unchanged_document_is_written_back_as_is(self):
        src = read(BUDGET)
        self.assertEqual(markdown.write(markdown.parse(src)), src)

    def test_tables_and_fences(self):
        src = "# T\n\n```layer x\n| a | b |\n|---|---|\n```\n\n## Costs :: matrix fill\n\n| a | b:Euro |\n|---|---:|\n| 1 | x \\| y |\n"
        segs = markdown.parse(src)
        tables = [s for s in segs if isinstance(s, markdown.Table)]
        self.assertEqual(len(tables), 1, "tables inside fences are text")
        t = tables[0]
        self.assertEqual((t.level, t.name, t.shape), (2, "Costs", "matrix fill"))
        self.assertEqual(t.header, ["a", "b:Euro"])
        self.assertEqual(t.aligns, ["", "right"])
        self.assertEqual(t.rows, [["1", "x | y"]])

    def test_canonical_table(self):
        t = markdown.Table(2, "A", "matrix", ["Item", "Amount:Euro"], ["", "right"], [["Rent", "1200,00"], ["x|y", "5"]])
        self.assertEqual(markdown.write_table(t), "\n".join([
            "## A :: matrix",
            "",
            "| Item | Amount:Euro |",
            "|------|------------:|",
            "| Rent |     1200,00 |",
            "| x\\|y |           5 |",
        ]))


class ToCalc(unittest.TestCase):
    def setUp(self):
        src = read(BUDGET)
        self.plans, self.state = convert.plan_import(src)
        self.cells = dict((p.sheet, p.cells) for p in self.plans)

    def f(self, sheet, r, c):
        return self.cells[sheet][r][c]

    def test_formulas(self):
        self.assertEqual(self.f("Expenses", 0, 3), ("f", "=B2/SUM($B$2:$B$4)"))
        self.assertEqual(self.f("Expenses", 0, 4), ("f", "=B2"), "a column outside aggregates is the own row")
        self.assertEqual(self.f("Expenses", 2, 4), ("f", "=B4*119%"))
        self.assertEqual(self.f("Metrics", 0, 1), ("f", "=SUM($'Expenses'.$E$2:$E$4)"))
        self.assertEqual(self.f("Metrics", 1, 1), ("f", "=$'Expenses'.B2"))
        self.assertEqual(self.f("Metrics", 2, 1), ("f", "=SUMIFS($'Sales'.$D$2:$D$5;$'Sales'.$B$2:$B$5;\"North\")"))
        self.assertEqual(self.f("Metrics", 3, 1), ("f", "=B4-B2"))
        self.assertEqual(self.f("Metrics", 4, 1), ("f", "=12*B7"), "row label → value column")

    def test_xlookup_is_written_without_xlookup(self):
        """XLOOKUP exists in Calc 24.8 and later only; older versions must work too."""
        book = Book([TableInfo("T", "T", ["Key", "Val"], 3, ["a", "b", "c"])])
        self.assertEqual(to_calc('=xlookup("b", Key, Val)', book, 0, 0), '=INDEX($B$2:$B$4;MATCH("b";$A$2:$A$4;0))')
        self.assertEqual(to_calc('=xlookup("z", Key, Val, 0)', book, 0, 0), '=IFNA(INDEX($B$2:$B$4;MATCH("z";$A$2:$A$4;0));0)')

    def test_untranslatable_formula_stays_text(self):
        self.assertEqual(self.f("Metrics", 6, 1), ("s", "=spark(Expenses!Gross)"))
        plan = [p for p in self.plans if p.sheet == "Metrics"][0]
        self.assertEqual(plan.notes, [(7, 1, convert.NOTE_KEPT)])

    def test_values(self):
        self.assertEqual(self.f("Expenses", 0, 1), ("n", 1200.0))
        self.assertEqual(self.f("Expenses", 0, 2), ("s", "paid"))
        self.assertEqual(self.f("Metrics", 5, 1), ("s", "5 m/s"))
        ausgaben = [p for p in self.plans if p.sheet == "Expenses"][0]
        self.assertEqual(ausgaben.decimals, {1: 2, 3: 3, 4: 2})

    def test_user_functions_are_inlined(self):
        src = "```functions\ngross(x) = x * 1.19\n```\n\n## T :: matrix\n\n| a | b |\n|---|---|\n| 2 | =gross(a) |\n"
        plans, _ = convert.plan_import(src)
        self.assertEqual(plans[0].cells[0][1], ("f", "=(A2*1.19)"))

    def test_text_columns_and_dates(self):
        src = "```types\nPLZ = text\n```\n\n## T :: matrix\n\n| plz:PLZ | Tag |\n|---|---|\n| 01069 | 2026-10-08 |\n"
        plans, _ = convert.plan_import(src)
        self.assertEqual(plans[0].cells[0], [("s", "01069"), ("d", 46303.0)])
        self.assertEqual(plans[0].dates, {1})

    def test_sheet_names(self):
        src = "## A/B :: matrix\n\n| x |\n|---|\n| 1 |\n\n| y |\n|---|\n| 2 |\n"
        plans, _ = convert.plan_import(src)
        self.assertEqual([p.sheet for p in plans], ["A_B", "Table 1"])


class FromCalc(unittest.TestCase):
    def setUp(self):
        self.book = Book([
            TableInfo("Expenses", "Expenses", ["Item", "Amount", "Status"], 3, ["Rent", "Power", "Internet"]),
            TableInfo("Meine Costs", "Meine Costs", ["A", "B"], 5),
        ])

    def t(self, f, here=0):
        return from_calc(f, self.book, here)

    def test_references_shift_by_the_header(self):
        self.assertEqual(self.t("=B2*1.19"), "=B1*1.19")
        self.assertEqual(self.t("=$B$2+B$3"), "=$B$1+B$2")
        self.assertEqual(self.t("=SUM(B2:B3)"), "=sum(B1:B2)")
        self.assertEqual(self.t("=SUM($B$2:$B$4)"), "=sum(B)", "the whole data column")
        self.assertEqual(self.t("=SUM(B2:B100)"), "=sum(B)", "open end below the data")
        self.assertEqual(self.t("=SUM(B:B)"), "=sum(B)")
        self.assertEqual(self.t("=B2:B4*2"), "=B1:B3*2", "outside functions a range stays a range")
        self.assertEqual(self.t("=IF(A2=\"x\";TRUE();FALSE())"), "=if(A1=\"x\",true,false)")
        self.assertEqual(self.t("=ROUND(B3;2)"), "=round(B2,2)")

    def test_new_functions(self):
        self.assertEqual(self.t("=ISBLANK(B2)"), "=isblank(B1)")
        self.assertEqual(self.t("=EDATE(B2;1)"), "=edate(B1,1)")
        self.assertEqual(self.t("=DATEDIF(A2;B2;\"MD\")"), "=datedif(A1,B1,\"MD\")")
        self.assertEqual(self.t("=IFS(A2>1;1;TRUE();2)"), "=ifs(A1>1,1,true,2)")
        self.assertEqual(self.t("=MAXIFS(B2:B4;A2:A4;\"x\")"), "=maxifs(B,A,\"x\")", "whole data columns become the column")

    def test_other_sheets(self):
        self.assertEqual(self.t("=$'Meine Costs'.B2+1"), "=\"Meine Costs\"!B1+1")
        self.assertEqual(self.t("=SUM($'Meine Costs'.$B$2:$B$6)"), "=sum(\"Meine Costs\"!B)")
        self.assertEqual(self.t("=$Expenses.B2", here=1), "=Expenses!B1")
        self.assertEqual(self.t("=$Expenses.B2:$Expenses.B3", here=1), "=Expenses!B1:B2")

    def test_untranslatable(self):
        for f in ("=VLOOKUP(A2;A2:C4;2;0)", "=B1", "=Z9", "=SUM(1:1)", "=$Unknown.A1", "=MyName*2", "={1;2}"):
            with self.assertRaises(FormulaError, msg=f):
                self.t(f)

    def test_round_trip_of_translated_formulas(self):
        book = Book([TableInfo("T", "T", ["a", "b"], 4, [])])
        for hc in ("=B1/sum(B)", "=A1*2+B2", "=(A1+B1)*2", "=A1-(B1-B2)", "=A1^(2^3)",
                   "=sumif(A, \">1\", B)", "=round(A1, 2)"):
            calc_f = to_calc(hc, book, 0, 0)
            self.assertEqual(from_calc(calc_f, book, 0).replace(" ", ""), hc.replace(" ", ""), calc_f)


class Export(unittest.TestCase):
    def setUp(self):
        self.src = read(BUDGET)
        self.plans, self.state = convert.plan_import(self.src)
        self.sheets = calc(self.plans, self.state)

    def test_unchanged(self):
        self.assertEqual(convert.export(self.sheets, self.state), self.src)

    def test_changed_value_rewrites_only_its_table(self):
        self.sheets[0].values[2][1] = 90.25
        out = convert.export(self.sheets, self.state)
        self.assertIn("|       90.25 | open ", out)
        self.assertIn("|     1200.00 |", out, "the column keeps two decimals")
        self.assertIn("=Amount * 1.19", out, "unchanged formulas keep their spelling")
        rest = out.split("## Sales", 1)[1]
        self.assertEqual(rest, self.src.split("## Sales", 1)[1])

    def test_new_row_and_formula(self):
        a = self.sheets[0]
        a.values.append(["Water", 20.0, "", 0.0, ""])
        a.formulas[(4, 3)] = "=B5/SUM($B$2:$B$5)"
        out = convert.export(self.sheets, self.state)
        self.assertIn("| Water    |       20.00 |               | =B4/sum(B)       |", out)

    def test_new_sheet_and_untranslatable_formula(self):
        self.sheets.append(convert.SheetData("New", [["Name", "Value"], ["a", 1.5], ["", 7.0]],
                                             {(2, 1): "=VLOOKUP(1;A2:B3;2;0)"}))
        out = convert.export(self.sheets, self.state)
        self.assertTrue(out.startswith(self.src.rstrip("\n")))
        self.assertIn("## New :: matrix\n\n| Name | Value |\n|------|-------|\n| a    | 1.5   |\n|      | 7     |\n", out)
        self.assertIn("```layer comments @New\nB2: original formula: =VLOOKUP(1;A2:B3;2;0)\n```\n", out)

    def test_saving_twice_gives_the_same_text(self):
        self.sheets.append(convert.SheetData("New", [["Name", "Value"], ["a", 1.5], ["", 7.0]],
                                             {(2, 1): "=VLOOKUP(1;A2:B3;2;0)"}))
        first = convert.export(self.sheets, self.state)
        plans, state = convert.plan_import(first)
        sheets = calc(plans, state)
        sheets[-1].formulas[(2, 1)] = "=VLOOKUP(1;A2:B3;2;0)"
        self.assertEqual(convert.export(sheets, state), first)

    def test_renamed_and_deleted_sheets(self):
        self.sheets[1].name = "Revenue"
        del self.sheets[2]  # Metrics
        out = convert.export(self.sheets, self.state)
        self.assertIn("## Revenue :: tensor[Month, Region, Product]", out)
        self.assertNotIn("Metrics", out)
        self.assertIn("```chart SalesTrend", out, "text around tables stays")

    def test_without_state(self):
        sheets = [convert.SheetData("Sheet1", [["Name", "Amount"], ["Rent", 1200.5], ["Total", 0.0]],
                                    {(2, 1): "=SUM(B2:B2)"})]
        self.assertEqual(convert.export(sheets, None), "\n".join([
            "## Sheet1 :: matrix",
            "",
            "| Name  | Amount      |",
            "|-------|-------------|",
            "| Rent  | 1200.5      |",
            "| Total | =sum(B1:B1) |",
            "",
        ]))

    def test_crlf_is_kept(self):
        src = "# T\r\n\r\n| a |\r\n|---|\r\n| 1 |\r\n"
        plans, state = convert.plan_import(src)
        sheets = calc(plans, state)
        sheets[0].values[1][0] = 2.0
        self.assertEqual(convert.export(sheets, state), "# T\r\n\r\n| a   |\r\n|-----|\r\n| 2   |\r\n")


class Styles(unittest.TestCase):
    COLS = ["Name", "Value", "Status"]

    def grid(self, rows=3):
        return [[styles.DEFAULT] * 3 for _ in range(rows)]

    def test_rules_to_grid(self):
        rules = [("B", "align right bold"), ("D", "percent(1)"), ("row[Status=open]", "fg yellow"),
                 ("B2", "nobold fg #102030"), ("Name", "italic"), ("*", "bg lightyellow")]
        g = styles.grid_of(rules, self.COLS, 3)
        bold, italic, underline, fg, bg, align = g[0][1]
        self.assertEqual((bold, italic, fg, bg, align), (True, False, None, 0xFFFFE0, "right"))
        self.assertEqual(g[1][1][:4], (False, False, False, 0x102030), "the cell rule wins")
        self.assertTrue(g[2][0][1], "column by name")
        self.assertEqual([op[:4] for op in styles.operations(rules, self.COLS, 3)][:2], [(0, 2, 1, 1), (1, 1, 1, 1)])

    def test_grid_to_short_rules(self):
        g = self.grid(4)
        for r in (0, 2, 3):
            g[r][1] = (True,) + g[r][1][1:]         # B mostly bold, B2 not
        for r in (0, 1):
            g[r][0] = g[r][0][:3] + (0xFF0000,) + g[r][0][4:]   # A1:A2 red (half: no column rule)
        for c in range(3):
            g[2][c] = (g[2][c][0], True) + g[2][c][2:]          # row 3 italic
        self.assertEqual(styles.rules_of(g, 3), [
            ("B", "bold"),
            ("3", "italic"),
            ("A1:A2", "fg red"),
            ("B2", "nobold"),
        ])
        self.assertEqual(styles.grid_of(styles.rules_of(g, 3), self.COLS, 4), g, "the rules give the grid back")

    def test_random_grids_round_trip(self):
        import random
        rnd = random.Random(7)
        choices = [(False, True), (False, True), (False, True), (None, 0xFF0000, 0x123456),
                   (None, 0xFFFF00), (None, "left", "right", "center")]
        for _ in range(300):
            rows, cols = rnd.randint(1, 6), rnd.randint(1, 4)
            base = [rnd.choice(c) if rnd.random() < 0.3 else c[0] for c in choices]
            g = [[tuple(rnd.choice(c) if rnd.random() < 0.25 else base[i] for i, c in enumerate(choices))
                  for _ in range(cols)] for _ in range(rows)]
            columns = ["c%d" % k for k in range(cols)]
            rules = styles.rules_of(g, cols)
            back = styles.grid_of(rules, columns, rows)
            # Alignment cannot be switched back to "standard" by a rule: only compare where it is set.
            for r in range(rows):
                for c in range(cols):
                    want, got = g[r][c], back[r][c]
                    self.assertEqual(want[:5], got[:5], (g, rules))
                    if want[5] is not None:
                        self.assertEqual(want[5], got[5], (g, rules))

    def test_new_layer_keeps_what_calc_cannot_show(self):
        old = [("B", "align right"), ("D", "percent(1)"), ("row[Status=open]", "fg yellow bold")]
        cols = ["Item", "Amount", "Status", "Share"]
        g = styles.grid_of(old, cols, 2)
        self.assertIsNone(styles.new_layer(old, g, cols), "unchanged")
        g[0][0] = (True,) + g[0][0][1:]
        g[1][0] = (True,) + g[1][0][1:]
        self.assertEqual(styles.new_layer(old, g, cols), [
            ("A", "bold"), ("B", "align right"), ("D", "percent(1)"), ("row[Status=open]", "fg yellow bold")])

    def test_colors(self):
        self.assertEqual(styles.color_value("Red"), 0xFF0000)
        self.assertEqual(styles.color_value("#0a0B0c"), 0x0A0B0C)
        self.assertIsNone(styles.color_value("chartreuse"))
        self.assertEqual(styles.color_text(0x00B050), "green")
        self.assertEqual(styles.color_text(0x123456), "#123456")


class ExportFormats(unittest.TestCase):
    def setUp(self):
        self.src = read(BUDGET)
        self.plans, self.state = convert.plan_import(self.src)
        self.sheets = calc(self.plans, self.state)

    def test_import_applies_the_format_layer(self):
        ops = self.plans[0].format_ops
        self.assertEqual(ops, [(0, 2, 1, 1, ["align right"])], "percent and row[…] stay in hardcalc")

    def test_unchanged_formatting_keeps_the_file(self):
        self.assertEqual(convert.export(self.sheets, self.state), self.src)

    def test_calc_formatting_replaces_the_layer(self):
        a = self.sheets[0]
        for r in (1, 2, 3):
            set_style(a, r, 0, bold=True)
        set_style(a, 2, 4, fg=0xFF0000)
        out = convert.export(self.sheets, self.state)
        self.assertIn("```layer format\nA: bold\nB: align right\nE2: fg red\nD: percent(1)\nrow[Status=open]: fg yellow\n```", out)
        self.assertEqual(out.replace(out[out.index("```layer format"):out.index("```layer validate")], ""),
                         self.src.replace(self.src[self.src.index("```layer format"):self.src.index("```layer validate")], ""),
                         "only the format block changes")

    def test_new_block_after_the_table_and_removal(self):
        u = self.sheets[1]  # Sales, no format layer
        set_style(u, 1, 3, bold=True)
        out = convert.export(self.sheets, self.state)
        self.assertIn("| Feb   | South  | B       | 42.10      |\n\n```layer format\nD1: bold\n```\n\n```chart SalesTrend", out)
        # Saved, opened again, the formatting removed: the block goes again.
        plans, state = convert.plan_import(out)
        sheets = calc(plans, state)
        self.assertEqual(sheets[1].styles[1][3][0], True)
        set_style(sheets[1], 1, 3, bold=False)
        self.assertEqual(convert.export(sheets, state), self.src)


if __name__ == "__main__":
    unittest.main()
