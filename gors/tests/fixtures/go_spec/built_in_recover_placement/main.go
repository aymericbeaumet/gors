package main

func helperCallsRecover() interface{} {
	return recover()
}

func indirectRecovery() (stopped bool, indirectSaw interface{}) {
	defer func() {
		// This deferred function calls recover directly, so it stops the
		// panicking sequence for real.
		stopped = recover() != nil
	}()
	defer func() {
		// recover is NOT called directly by this deferred function: the
		// helper's recover must return nil and must not stop panicking.
		indirectSaw = helperCallsRecover()
	}()
	panic("boom")
}

func main() {
	// recover called while the goroutine is not panicking returns nil.
	if recover() != nil {
		panic("recover returned non-nil while not panicking")
	}

	// recover called by a plain (non-deferred) nested call returns nil.
	notDeferred := func() interface{} {
		return recover()
	}
	notDeferredSaw := notDeferred()
	if notDeferredSaw != nil {
		panic("recover in non-deferred call returned non-nil")
	}

	stopped, indirectSaw := indirectRecovery()
	if !stopped {
		panic("directly deferred recover did not stop the panic")
	}
	if indirectSaw != nil {
		panic("indirect recover (not called directly by a deferred function) returned non-nil")
	}

	println("handling-panics-recover-placement: ok")
}
