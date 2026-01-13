package testfiles

// Test basic select statement
func basicSelect(ch1 chan int, ch2 chan string) {
	select {
	case x := <-ch1:
		_ = x
	case s := <-ch2:
		_ = s
	}
}

// Test select with default
func selectWithDefault(ch chan int) int {
	select {
	case x := <-ch:
		return x
	default:
		return -1
	}
}

// Test select with send
func selectWithSend(ch1 chan int, ch2 chan int, val int) {
	select {
	case ch1 <- val:
		return
	case ch2 <- val * 2:
		return
	}
}

// Test select with multiple cases
func selectMultiple(ch1 chan int, ch2 chan int, ch3 chan string) string {
	select {
	case <-ch1:
		return "ch1"
	case x := <-ch2:
		_ = x
		return "ch2"
	case s := <-ch3:
		return s
	default:
		return "default"
	}
}

// Test empty select (blocks forever)
func emptySelect() {
	select {}
}

// Test select with assignment operators
func selectAssign(ch chan int) int {
	var result int
	select {
	case result = <-ch:
		return result
	default:
		return 0
	}
}
