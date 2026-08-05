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
	panic("out-of-bounds panic continued after recovery")
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
	panic("nil pointer panic continued after recovery")
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
	panic("divide-by-zero panic continued after recovery")
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
	panic("generic string index panic continued after recovery")
}

func testGenericByteSliceIndexOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic byte slice index panic was not recovered")
		}
	}()
	triggerGenericByteIndex([]byte{1})
	panic("generic byte slice index panic continued after recovery")
}

func testGenericStringSliceOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic string slice panic was not recovered")
		}
	}()
	triggerGenericByteSlice("\xff")
	panic("generic string slice panic continued after recovery")
}

func testGenericByteSliceSliceOutOfBounds() {
	defer func() {
		if r := recover(); r == nil {
			panic("generic byte slice slice panic was not recovered")
		}
	}()
	triggerGenericByteSlice([]byte{1})
	panic("generic byte slice slice panic continued after recovery")
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
