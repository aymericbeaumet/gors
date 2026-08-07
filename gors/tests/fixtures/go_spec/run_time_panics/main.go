package main

func triggerOutOfBounds(s []int, idx int) {
	_ = s[idx]
}

func testOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("out-of-bounds panic was not recovered")
		}
	}()
	s := []int{1}
	triggerOutOfBounds(s, 2)
}

func triggerNilPointer(p *int) {
	_ = *p
}

func testNilPointer() {
	defer func() {
		if r := recover(); r == nil {
			panic("nil pointer panic was not recovered")
		}
	}()
	triggerNilPointer(nil)
}

func triggerDivideByZero(a, b int) {
	_ = a / b
}

func testDivideByZero() {
	defer func() {
		if r := recover(); r == nil {
			panic("divide-by-zero panic was not recovered")
		}
	}()
	triggerDivideByZero(1, 0)
}

func triggerGenericByteIndex[S ~string | ~[]byte](value S) {
	_ = value[len(value)]
}

func triggerGenericByteSlice[S ~string | ~[]byte](value S) {
	_ = value[:len(value)+1]
}

func testGenericStringIndexOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic string index panic was not recovered")
		}
	}()
	triggerGenericByteIndex("\xff")
}

func testGenericByteSliceIndexOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic byte slice index panic was not recovered")
		}
	}()
	triggerGenericByteIndex([]byte{1})
}

func testGenericStringSliceOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic string slice panic was not recovered")
		}
	}()
	triggerGenericByteSlice("\xff")
}

func testGenericByteSliceSliceOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic byte slice slice panic was not recovered")
		}
	}()
	triggerGenericByteSlice([]byte{1})
}

func main() {
	testOutOfBounds()
	testNilPointer()
	testDivideByZero()
	testGenericStringIndexOutOfBounds()
	testGenericByteSliceIndexOutOfBounds()
	testGenericStringSliceOutOfBounds()
	testGenericByteSliceSliceOutOfBounds()
}
