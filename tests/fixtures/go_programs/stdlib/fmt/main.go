package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Println("== fmt/append ==")
	case_fmt_append()
	fmt.Println("== fmt/appendf ==")
	case_fmt_appendf()
	fmt.Println("== fmt/appendln ==")
	case_fmt_appendln()
	fmt.Println("== fmt/errorf ==")
	case_fmt_errorf()
	fmt.Println("== fmt/fprint ==")
	case_fmt_fprint()
	fmt.Println("== fmt/fprintf ==")
	case_fmt_fprintf()
	fmt.Println("== fmt/fprintln ==")
	case_fmt_fprintln()
	fmt.Println("== fmt/print ==")
	case_fmt_print()
	fmt.Println("== fmt/printf ==")
	case_fmt_printf()
	fmt.Println("== fmt/println ==")
	case_fmt_println()
	fmt.Println("== fmt/sprint ==")
	case_fmt_sprint()
	fmt.Println("== fmt/sprintf ==")
	case_fmt_sprintf()
	fmt.Println("== fmt/sprintln ==")
	case_fmt_sprintln()
}

func case_fmt_append() {
	out := []byte("start:")
	out = fmt.Append(out, "value", 7)
	fmt.Println(string(out))
}

func case_fmt_appendf() {
	out := []byte("start:")
	out = fmt.Appendf(out, "%s=%d", "value", 7)
	fmt.Println(string(out))
}

func case_fmt_appendln() {
	out := []byte("start:")
	out = fmt.Appendln(out, "value", 7)
	fmt.Print(string(out))
}

func case_fmt_errorf() {
	err := fmt.Errorf("value %d failed", 7)
	fmt.Println(err)
}

func case_fmt_fprint() {
	fmt.Fprint(os.Stdout, "value", 7)
	fmt.Println("")
}

func case_fmt_fprintf() {
	fmt.Fprintf(os.Stdout, "%s=%d\n", "value", 7)
}

func case_fmt_fprintln() {
	fmt.Fprintln(os.Stdout, "value", 7)
}

func case_fmt_print() {
	fmt.Print("value", 7)
	fmt.Println("")
}

func case_fmt_printf() {
	fmt.Printf("%s=%d %c %.2f\n", "value", 7, 65, 3.5)
}

func case_fmt_println() {
	fmt.Println("value", 7, true)
}

func case_fmt_sprint() {
	out := fmt.Sprint("value", 7)
	fmt.Println(out)
}

func case_fmt_sprintf() {
	out := fmt.Sprintf("%s=%d %c %.2f", "value", 7, 65, 3.5)
	fmt.Println(out)
}

func case_fmt_sprintln() {
	out := fmt.Sprintln("value", 7, true)
	fmt.Print(out)
}
