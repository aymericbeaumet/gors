package main

func pair() (int, int) {
	return 3, 4
}

func returnCall() (int, int) {
	return pair()
}

func namedReturn() (left int, right int) {
	left = 5
	right = 6
	return
}

func deferredNamedReturn() (result int) {
	defer func() {
		result += 2
	}()
	return 7
}

func main() {
	callLeft, callRight := returnCall()
	namedLeft, namedRight := namedReturn()
	if callLeft != 3 || callRight != 4 {
		panic("call return values changed")
	}
	if namedLeft != 5 || namedRight != 6 {
		panic("named return values changed")
	}
	if deferredNamedReturn() != 9 {
		panic("deferred named return changed")
	}
}
