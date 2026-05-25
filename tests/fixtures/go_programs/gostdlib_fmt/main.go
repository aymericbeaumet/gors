package main

import (
	"fmt"
	"os"
)

type point struct {
	X int
	Y int
}

func (p point) String() string {
	return "Point"
}

func double(n int) int {
	return n * 2
}

func add(a int, b int) int {
	return a + b
}

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
	fmt.Println("== fmt/printf_formats ==")
	case_fmt_printf_formats()
	fmt.Println("== fmt/println ==")
	case_fmt_println()
	fmt.Println("== fmt/sprint ==")
	case_fmt_sprint()
	fmt.Println("== fmt/sprintf ==")
	case_fmt_sprintf()
	fmt.Println("== fmt/sprintln ==")
	case_fmt_sprintln()
	fmt.Println("== fmt/stringer ==")
	case_fmt_stringer()
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
	fmt.Fprintf(os.Stdout, "Hello, %s!\n", "World")
	fmt.Fprintf(os.Stdout, "Number: %d\n", 42)
}

func case_fmt_fprintln() {
	fmt.Fprintln(os.Stdout, "value", 7)
}

func case_fmt_print() {
	fmt.Print("value", 7)
	fmt.Println("")
	fmt.Print("a")
	fmt.Print("b")
	fmt.Print("c")
	fmt.Println("")
	fmt.Print("x")
	fmt.Println("y")
}

func case_fmt_printf() {
	fmt.Printf("%s=%d %c %.2f\n", "value", 7, 65, 3.5)
	fmt.Printf("Hello, %s!\n", "World")
	fmt.Printf("%d + %d = %d\n", 1, 2, 3)
	fmt.Printf("%c\n", 65)
	fmt.Printf("%c%c%c\n", 72, 105, 33)
	fmt.Printf("%d is %c\n", 90, 90)
}

func case_fmt_printf_formats() {
	fmt.Printf("%t %b %o %O %x %X\n", true, 10, 10, 10, 255, 255)
	fmt.Printf("%q %U\n", 'A', 'A')
	fmt.Printf("%s %q %x % X\n", []byte("go"), []byte("go"), []byte("go"), []byte("go"))
	fmt.Println(fmt.Sprintf("%[2]d %[1]d", 11, 22))
	fmt.Println(fmt.Sprintf("%08d %.3s", 42, "gopher"))
}

func case_fmt_println() {
	fmt.Println("value", 7, true)
	fmt.Println("before")
	fmt.Println()
	fmt.Println("after")

	x := 10
	y := 20
	fmt.Println(x + y)
	fmt.Println(x * y)
	fmt.Println(x - y)
	fmt.Println(100 / 4)
	fmt.Println(17 % 5)

	fmt.Println(double(21))
	fmt.Println(add(3, 4))
	fmt.Println(double(add(5, 10)))

	for i := 0; i < 5; i++ {
		fmt.Println(i)
	}
	fmt.Println("done")

	fmt.Println("a", "b", "c", "d", "e")
	fmt.Println(1, 2, 3, 4, 5, 6)

	fmt.Println("hello")
	fmt.Println(42)
	fmt.Println(-7)
	fmt.Println(0)
	fmt.Println(true)
	fmt.Println(false)
	fmt.Println(999)
	fmt.Println(3)
	fmt.Println("world")
}

func case_fmt_sprint() {
	out := fmt.Sprint("value", 7)
	fmt.Println(out)
	fmt.Println(fmt.Sprint("sprint", ":", 4))
}

func case_fmt_sprintf() {
	out := fmt.Sprintf("%s=%d %c %.2f", "value", 7, 65, 3.5)
	fmt.Println(out)
	fmt.Println(fmt.Sprintf("sprintf:%s:%d", "ok", 5))

	name := "World"
	s := fmt.Sprintf("Hello, %s!", name)
	fmt.Println(s)

	n := 42
	s2 := fmt.Sprintf("The answer is %d", n)
	fmt.Println(s2)
}

func case_fmt_sprintln() {
	out := fmt.Sprintln("value", 7, true)
	fmt.Print(out)
	fmt.Print(fmt.Sprintln("sprintln", false, 6))
}

func case_fmt_stringer() {
	p := point{X: 1, Y: 2}
	fmt.Println(p)
}
