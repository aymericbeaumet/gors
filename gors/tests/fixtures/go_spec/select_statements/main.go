package main

func main() {
	sendCh := make(chan int, 1)
	sendResult := 0
	select {
	case sendCh <- 4:
		sendResult = <-sendCh
	default:
		sendResult = -1
	}

	fullCh := make(chan int, 1)
	fullCh <- 9
	defaultSend := 0
	select {
	case fullCh <- 10:
		defaultSend = -1
	default:
		defaultSend = <-fullCh
	}

	recvCh := make(chan int, 1)
	recvCh <- 5
	recvResult := 0
	select {
	case recvResult = <-recvCh:
	default:
		recvResult = -1
	}

	emptyCh := make(chan int, 1)
	defaultRecv := 0
	select {
	case defaultRecv = <-emptyCh:
		defaultRecv = -1
	default:
		defaultRecv = 7
	}

	closedCh := make(chan int, 1)
	close(closedCh)
	closedValue := -1
	closedOK := true
	select {
	case closedValue, closedOK = <-closedCh:
	default:
		closedValue = -2
	}

	bufferedClosedCh := make(chan int, 1)
	bufferedClosedCh <- 11
	close(bufferedClosedCh)
	bufferedValue := 0
	bufferedOK := false
	select {
	case value, ok := <-bufferedClosedCh:
		bufferedValue = value
		bufferedOK = ok
	default:
		bufferedValue = -1
	}

	drainedValue := -1
	drainedOK := true
	select {
	case value, ok := <-bufferedClosedCh:
		drainedValue = value
		drainedOK = ok
	default:
		drainedValue = -2
	}

	if sendResult != 4 || defaultSend != 9 {
		panic("select send behavior changed")
	}
	if recvResult != 5 || defaultRecv != 7 {
		panic("select receive behavior changed")
	}
	if closedValue != 0 || closedOK {
		panic("closed channel select behavior changed")
	}
	if bufferedValue != 11 || !bufferedOK || drainedValue != 0 || drainedOK {
		panic("buffered closed channel select changed")
	}
}
