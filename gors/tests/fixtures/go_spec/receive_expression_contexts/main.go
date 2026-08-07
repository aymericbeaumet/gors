package main



type flag bool

func double(v int) int { return v * 2 }

func main() {
	ch := make(chan int, 4)
	ch <- 1
	ch <- 2
	ch <- 3
	ch <- 4

	println("call", double(<-ch)) // receive as a call argument
	// Receive operations in the operands of an expression are evaluated
	// in lexical left-to-right order: (<-ch) + ((<-ch) * 10)
	sum := <-ch + <-ch*10
	println("sum", sum)
	<-ch // receive as a statement: value discarded
	println("len", len(ch))

	closed := make(chan int, 1)
	closed <- 9
	close(closed)
	var v int
	var ok flag // the comma-ok result is an untyped boolean:
	v, ok = <-closed
	println("first", v, bool(ok))
	v, ok = <-closed
	println("second", v, bool(ok))
}
