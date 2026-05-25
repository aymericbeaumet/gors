package main

import "fmt"

func sink(x any) {}

func main() {
	fmt.Println("== builtin/any_parameter ==")
	case_builtin_any_parameter()
	fmt.Println("== builtin/any_type ==")
	case_builtin_any_type()
	fmt.Println("== builtin/append_copy ==")
	case_builtin_append_copy()
	fmt.Println("== builtin/append_variadic ==")
	case_builtin_append_variadic()
	fmt.Println("== builtin/close ==")
	case_builtin_close()
	fmt.Println("== builtin/complex ==")
	case_builtin_complex()
	fmt.Println("== builtin/delete_clear ==")
	case_builtin_delete_clear()
	fmt.Println("== builtin/len_cap ==")
	case_builtin_len_cap()
	fmt.Println("== builtin/len_cap_maps_arrays_channels ==")
	case_builtin_len_cap_maps_arrays_channels()
	fmt.Println("== builtin/make_new ==")
	case_builtin_make_new()
	fmt.Println("== builtin/make_map_chan ==")
	case_builtin_make_map_chan()
	fmt.Println("== builtin/max_min ==")
	case_builtin_max_min()
}

func case_builtin_any_parameter() {
	sink("value")
	fmt.Println("ok")
}

func case_builtin_any_type() {
	var x any
	_ = x
	fmt.Println("any type works")
}

func case_builtin_append_copy() {
	s := []int{1, 2, 3}
	s = append(s, 4)
	fmt.Println(len(s))

	src := []int{10, 20, 30}
	dst := make([]int, 5)
	n := copy(dst, src)
	fmt.Println(n)
	fmt.Println(len(dst))
}

func case_builtin_append_variadic() {
	values := []int{1, 2}
	more := []int{3, 4}
	values = append(values, more...)
	fmt.Println(values)

	bytes := []byte("go")
	bytes = append(bytes, "rs"...)
	fmt.Println(string(bytes))

	dst := make([]byte, 5)
	n := copy(dst, "hi")
	fmt.Println(n)
	fmt.Println(string(dst[:n]))
}

func case_builtin_close() {
	ch := make(chan int, 2)
	ch <- 7
	close(ch)
	value, ok := <-ch
	fmt.Println(value, ok)
	value, ok = <-ch
	fmt.Println(value, ok)
}

func case_builtin_complex() {
	c := complex(3.0, 4.0)
	fmt.Println(real(c))
	fmt.Println(imag(c))

	c2 := complex(1.0, 2.0)
	sum := c + c2
	fmt.Println(real(sum))
	fmt.Println(imag(sum))
}

func case_builtin_delete_clear() {
	m := map[string]int{"a": 1, "b": 2, "c": 3}
	delete(m, "b")
	fmt.Println(len(m))

	m2 := map[string]int{"x": 10, "y": 20}
	clear(m2)
	fmt.Println(len(m2))

	s := []int{1, 2, 3, 4, 5}
	clear(s)
	fmt.Println(s)
}

func case_builtin_len_cap() {
	s := []int{1, 2, 3, 4, 5}
	fmt.Println(len(s))

	str := "hello"
	fmt.Println(len(str))

	s2 := make([]int, 3)
	fmt.Println(len(s2))
	fmt.Println(cap(s2))
}

func case_builtin_len_cap_maps_arrays_channels() {
	arr := [3]int{1, 2, 3}
	fmt.Println(len(arr))
	fmt.Println(cap(arr))

	m := map[string]int{"a": 1, "b": 2}
	fmt.Println(len(m))

	ch := make(chan int, 4)
	ch <- 1
	ch <- 2
	fmt.Println(len(ch))
	fmt.Println(cap(ch))
}

func case_builtin_make_new() {
	s := make([]int, 5)
	fmt.Println(len(s))
	fmt.Println(cap(s))

	s2 := make([]int, 3, 10)
	fmt.Println(len(s2))
	fmt.Println(cap(s2))

	p := new(int)
	fmt.Println(*p)
}

func case_builtin_make_map_chan() {
	m := make(map[string]int)
	m["a"] = 1
	fmt.Println(len(m))

	m2 := make(map[string]int, 4)
	m2["x"] = 10
	fmt.Println(len(m2))

	ch := make(chan string, 2)
	ch <- "a"
	fmt.Println(len(ch))
	fmt.Println(cap(ch))
	close(ch)
}

func case_builtin_max_min() {
	fmt.Println(max(3, 7))
	fmt.Println(max(10, 2))
	fmt.Println(max(3, 7, 5))

	fmt.Println(min(3, 7))
	fmt.Println(min(10, 2))
	fmt.Println(min(3, 7, 5))

	fmt.Println(max(3.14, 2.71))
	fmt.Println(min(3.14, 2.71))

	fmt.Println(max("apple", "banana"))
	fmt.Println(min("apple", "banana"))
}
