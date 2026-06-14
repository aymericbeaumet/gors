package main

func safe() {
	defer func() {
		if recover() == nil {
			panic("deferred recover did not observe panic")
		}
	}()
	panic("boom")
	panic("function continued after recovered panic")
}

func main() {
	safe()
}
