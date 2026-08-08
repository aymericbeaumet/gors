package main



const (
	bit0, mask0 = 1 << iota, 1<<iota - 1 // iota == 0
	bit1, mask1                          // implicit repetition, iota == 1
	_, _                                 // iota == 2, skipped
	bit3, mask3                          // iota == 3
)

const (
	c0 = iota
	c1 = iota
	c2 = iota
)

// iota restarts at zero in each new const declaration.
const x = iota
const y = iota

const (
	u         = iota * 42 // untyped integer constant
	v float64 = iota * 42 // float64 constant
	w         = iota * 42 // untyped integer constant
)

func main() {
	println(bit0, mask0, bit1, mask1, bit3, mask3)
	println(c0, c1, c2, x, y)
	println(u, v, w)
	if _, ok := interface{}(v).(float64); !ok {
		panic("v must be float64")
	}
	if _, ok := interface{}(w).(int); !ok {
		panic("w must default to int")
	}
	if bit0 != 1 || mask0 != 0 || bit1 != 2 || mask1 != 1 || bit3 != 8 || mask3 != 7 {
		panic("parallel iota changed")
	}
	if c0 != 0 || c1 != 1 || c2 != 2 || x != 0 || y != 0 || u != 0 || v != 42.0 || w != 84 {
		panic("iota indexing changed")
	}
}
