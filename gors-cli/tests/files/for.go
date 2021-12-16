package main

func main() {
	// For statements with single condition
	for a < b {
		a *= 2
	}

	// For statements with for clause
	//for i := 0; i < 10; i++ {
	//f(i)
	//}
	//for ; i < 10; i++ {
	//f(i)
	//}
	//for i := 0; i < 10; {
	//f(i)
	//}
	//for ; i < 10; {
	//f(i)
	//}
	for i < 10 {
		f(i)
	}
	for cond {
		S()
	}
	for {
		S()
	}

	// For statements with range clause
	for i, _ := range testdata.a {
		// testdata.a is never evaluated; len(testdata.a) is constant
		// i ranges from 0 to 6
		f(i)
	}
	for w := range ch {
		doWork(w)
	}
	for range ch {
	}
}
