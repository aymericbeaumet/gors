package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Print("print", ":", 1, "\n")
	fmt.Printf("printf:%s:%d\n", "ok", 2)
	fmt.Println("println", true, 3)

	fmt.Println(fmt.Sprint("sprint", ":", 4))
	fmt.Println(fmt.Sprintf("sprintf:%s:%d", "ok", 5))
	fmt.Print(fmt.Sprintln("sprintln", false, 6))

	out := []byte("append:")
	out = fmt.Append(out, "value", 7)
	fmt.Println(string(out))

	out = []byte("appendf:")
	out = fmt.Appendf(out, "%s:%d", "value", 8)
	fmt.Println(string(out))

	out = []byte("appendln:")
	out = fmt.Appendln(out, "value", 9)
	fmt.Print(string(out))

	fmt.Fprint(os.Stdout, "fprint", ":", 10, "\n")
	fmt.Fprintf(os.Stdout, "fprintf:%s:%d\n", "ok", 11)
	fmt.Fprintln(os.Stdout, "fprintln", false, 12)

	err := fmt.Errorf("error:%s:%d", "ok", 13)
	fmt.Println(err)
}
