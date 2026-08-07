package main

func nested() (result string) {
	defer func() {
		r := recover()
		s, ok := r.(string)
		if !ok {
			panic("nested recover did not observe a string value")
		}
		print("[outer-defer:" + s + "]")
		result = s
	}()
	defer func() {
		print("[inner-defer]")
		panic("second")
	}()
	panic("first")
}

func main() {
	print("nested-panic-during-defer: ")
	got := nested()
	if got != "second" {
		panic("recover did not return the most recent panic value: " + got)
	}
	println(" ok")
}
