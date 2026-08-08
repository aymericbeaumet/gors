package main

func sendOnClosedPanics() (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	ch := make(chan int, 1)
	close(ch)
	ch <- 1 // send on closed channel: run-time panic
	return nil
}

func closeClosedPanics() (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	ch := make(chan int)
	close(ch)
	close(ch) // closing a closed channel: run-time panic
	return nil
}

func closeNilPanics() (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	var ch chan int
	close(ch) // closing the nil channel: run-time panic
	return nil
}

func main() {
	if r := sendOnClosedPanics(); r == nil {
		panic("send on closed channel did not panic")
	} else if _, ok := r.(error); !ok {
		panic("send-on-closed panic value does not satisfy error")
	}

	if r := closeClosedPanics(); r == nil {
		panic("close of closed channel did not panic")
	} else if _, ok := r.(error); !ok {
		panic("close-of-closed panic value does not satisfy error")
	}

	if r := closeNilPanics(); r == nil {
		panic("close of nil channel did not panic")
	} else if _, ok := r.(error); !ok {
		panic("close-of-nil panic value does not satisfy error")
	}

	// Receive from a closed channel never panics: it yields the zero value.
	ch := make(chan int, 1)
	ch <- 42
	close(ch)
	if v, ok := <-ch; !ok || v != 42 {
		panic("buffered value lost after close")
	}
	if v, ok := <-ch; ok || v != 0 {
		panic("receive from closed drained channel did not yield zero, false")
	}

	println("run-time-panics-channels: ok")
}
