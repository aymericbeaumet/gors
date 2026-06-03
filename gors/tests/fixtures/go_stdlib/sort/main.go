package main

import (
	"fmt"
	"sort"
)

type pair struct {
	Key  int
	Name string
}

type byKey []pair

var _ sort.Interface = sort.IntSlice([]int{1, 2, 3})

func (p byKey) Len() int {
	return len(p)
}

func (p byKey) Less(i int, j int) bool {
	return p[i].Key < p[j].Key
}

func (p byKey) Swap(i int, j int) {
	p[i], p[j] = p[j], p[i]
}

func main() {
	fmt.Println("== sort/find ==")
	case_sort_find()
	fmt.Println("== sort/find_not_found ==")
	case_sort_find_not_found()
	fmt.Println("== sort/float64_slice_len ==")
	case_sort_float64_slice_len()
	fmt.Println("== sort/float64_slice_less ==")
	case_sort_float64_slice_less()
	fmt.Println("== sort/float64_slice_search ==")
	case_sort_float64_slice_search()
	fmt.Println("== sort/float64_slice_sort ==")
	case_sort_float64_slice_sort()
	fmt.Println("== sort/float64_slice_swap ==")
	case_sort_float64_slice_swap()
	fmt.Println("== sort/float64s ==")
	case_sort_float64s()
	fmt.Println("== sort/float64s_are_sorted ==")
	case_sort_float64s_are_sorted()
	fmt.Println("== sort/int_slice_len ==")
	case_sort_int_slice_len()
	fmt.Println("== sort/int_slice_less ==")
	case_sort_int_slice_less()
	fmt.Println("== sort/int_slice_search ==")
	case_sort_int_slice_search()
	fmt.Println("== sort/int_slice_sort ==")
	case_sort_int_slice_sort()
	fmt.Println("== sort/int_slice_swap ==")
	case_sort_int_slice_swap()
	fmt.Println("== sort/ints ==")
	case_sort_ints()
	fmt.Println("== sort/ints_are_sorted ==")
	case_sort_ints_are_sorted()
	fmt.Println("== sort/is_sorted ==")
	case_sort_is_sorted()
	fmt.Println("== sort/reverse ==")
	case_sort_reverse()
	fmt.Println("== sort/search ==")
	case_sort_search()
	fmt.Println("== sort/search_float64s ==")
	case_sort_search_float64s()
	fmt.Println("== sort/search_ints ==")
	case_sort_search_ints()
	fmt.Println("== sort/search_strings ==")
	case_sort_search_strings()
	fmt.Println("== sort/slice ==")
	case_sort_slice()
	fmt.Println("== sort/slice_is_sorted ==")
	case_sort_slice_is_sorted()
	fmt.Println("== sort/slice_stable ==")
	case_sort_slice_stable()
	fmt.Println("== sort/sort ==")
	case_sort_sort()
	fmt.Println("== sort/stable ==")
	case_sort_stable()
	fmt.Println("== sort/stable_custom ==")
	case_sort_stable_custom()
	fmt.Println("== sort/method_expressions ==")
	case_sort_method_expressions()
	fmt.Println("== sort/string_slice_len ==")
	case_sort_string_slice_len()
	fmt.Println("== sort/string_slice_less ==")
	case_sort_string_slice_less()
	fmt.Println("== sort/string_slice_search ==")
	case_sort_string_slice_search()
	fmt.Println("== sort/string_slice_sort ==")
	case_sort_string_slice_sort()
	fmt.Println("== sort/string_slice_swap ==")
	case_sort_string_slice_swap()
	fmt.Println("== sort/strings ==")
	case_sort_strings()
	fmt.Println("== sort/strings_are_sorted ==")
	case_sort_strings_are_sorted()
	fmt.Println("== sort/basic_duplicates ==")
	case_sort_basic_duplicates()
}

func case_sort_find_compareToFive(i int) int {
	values := []int{1, 3, 5, 7}
	if 5 < values[i] {
		return -1
	}
	if 5 > values[i] {
		return 1
	}
	return 0
}

func case_sort_find() {
	idx, found := sort.Find(4, case_sort_find_compareToFive)
	fmt.Println(idx, found)
}

func case_sort_find_compareToSix(i int) int {
	values := []int{1, 3, 5, 7}
	if 6 < values[i] {
		return -1
	}
	if 6 > values[i] {
		return 1
	}
	return 0
}

func case_sort_find_not_found() {
	idx, found := sort.Find(4, case_sort_find_compareToSix)
	fmt.Println(idx, found)
}

func case_sort_float64_slice_len() {
	values := []float64{3.5, 1.25, 2.75}
	fmt.Println(sort.Float64Slice(values).Len())
}

func case_sort_float64_slice_less() {
	values := []float64{3.5, 1.25}
	fmt.Println(sort.Float64Slice(values).Less(1, 0))
}

func case_sort_float64_slice_search() {
	values := []float64{1.25, 3.5, 8.0}
	fmt.Println(sort.Float64Slice(values).Search(3.0))
}

func case_sort_float64_slice_sort() {
	values := []float64{3.5, -1.25, 0.5}
	sort.Float64Slice(values).Sort()
	fmt.Println(values)
}

func case_sort_float64_slice_swap() {
	values := []float64{1.25, 2.5, 3.75}
	sort.Float64Slice(values).Swap(0, 2)
	fmt.Println(values)
}

func case_sort_float64s() {
	values := []float64{3.5, -1.25, 0.5}
	sort.Float64s(values)
	if values[0] == -1.25 && values[2] == 3.5 {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

func case_sort_float64s_are_sorted() {
	values := []float64{-1.25, 0.5, 3.5}
	if sort.Float64sAreSorted(values) {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

func case_sort_int_slice_len() {
	values := []int{3, 1, 2}
	fmt.Println(sort.IntSlice(values).Len())
}

func case_sort_int_slice_less() {
	values := []int{3, 1, 2}
	fmt.Println(sort.IntSlice(values).Less(1, 0))
}

func case_sort_int_slice_search() {
	values := []int{1, 2, 3}
	fmt.Println(sort.IntSlice(values).Search(2))
}

func case_sort_int_slice_sort() {
	values := []int{3, 1, 2}
	sort.IntSlice(values).Sort()
	fmt.Println(values)
}

func case_sort_int_slice_swap() {
	values := []int{1, 2, 3}
	sort.IntSlice(values).Swap(0, 2)
	fmt.Println(values)
}

func case_sort_ints() {
	values := []int{3, 1, 2}
	sort.Ints(values)
	if values[0] == 1 && values[2] == 3 {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

func case_sort_ints_are_sorted() {
	values := []int{1, 2, 3}
	if sort.IntsAreSorted(values) {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

func case_sort_is_sorted() {
	values := []int{1, 2, 3}
	fmt.Println(sort.IsSorted(sort.IntSlice(values)))
}

func case_sort_reverse() {
	values := []int{1, 3, 2}
	sort.Sort(sort.Reverse(sort.IntSlice(values)))
	fmt.Println(values)
}

func case_sort_search_atLeastSeven(i int) bool {
	return i >= 7
}

func case_sort_search() {
	fmt.Println(sort.Search(10, case_sort_search_atLeastSeven))
	fmt.Println(sort.Search(0, case_sort_search_atLeastSeven))
}

func case_sort_search_float64s() {
	values := []float64{1.25, 3.5, 8.0}
	fmt.Println(sort.SearchFloat64s(values, 3.0))
}

func case_sort_search_ints() {
	values := []int{1, 3, 5, 7}
	fmt.Println(sort.SearchInts(values, 4))
}

func case_sort_search_strings() {
	values := []string{"alpha", "delta", "omega"}
	fmt.Println(sort.SearchStrings(values, "charlie"))
}

func case_sort_slice_keepOrder(i int, j int) bool {
	return i < j
}

func case_sort_slice() {
	values := []int{1, 2, 3}
	// gors:stdlib-cover sort::Slice
	sort.Slice(values, case_sort_slice_keepOrder)
	fmt.Println(values)
}

func case_sort_slice_is_sorted_keepOrder(i int, j int) bool {
	return i < j
}

func case_sort_slice_is_sorted() {
	values := []int{3, 2, 1}
	// gors:stdlib-cover sort::SliceIsSorted
	fmt.Println(sort.SliceIsSorted(values, case_sort_slice_is_sorted_keepOrder))
}

func case_sort_slice_stable_keepOrder(i int, j int) bool {
	return i < j
}

func case_sort_slice_stable() {
	values := []string{"a", "b", "c"}
	// gors:stdlib-cover sort::SliceStable
	sort.SliceStable(values, case_sort_slice_stable_keepOrder)
	fmt.Println(values)
}

func case_sort_sort() {
	values := []int{3, 1, 2}
	sort.Sort(sort.IntSlice(values))
	fmt.Println(values)

	records := byKey{{Key: 3, Name: "gamma"}, {Key: 1, Name: "alpha"}, {Key: 2, Name: "beta"}}
	sort.Sort(records)
	fmt.Println(records[0].Name, records[2].Name)
}

func case_sort_stable() {
	values := []string{"gamma", "alpha", "beta"}
	sort.Stable(sort.StringSlice(values))
	fmt.Println(values)
}

func case_sort_stable_custom() {
	records := byKey{{Key: 2, Name: "first"}, {Key: 1, Name: "middle"}, {Key: 2, Name: "second"}}
	sort.Stable(records)
	fmt.Println(records[0].Name, records[1].Name, records[2].Name)
}

func case_sort_method_expressions() {
	ints := []int{3, 1, 2}
	intView := sort.IntSlice(ints)
	fmt.Println((*sort.IntSlice).Len(&intView))
	fmt.Println((*sort.IntSlice).Less(&intView, 1, 0))
	fmt.Println((*sort.IntSlice).Search(&intView, 2))
	(*sort.IntSlice).Swap(&intView, 0, 2)
	fmt.Println(intView[0], intView[1], intView[2])
	(*sort.IntSlice).Sort(&intView)
	fmt.Println(intView[0], intView[1], intView[2])

	floats := []float64{3.5, -1.25, 0.5}
	floatView := sort.Float64Slice(floats)
	fmt.Println((*sort.Float64Slice).Len(&floatView))
	fmt.Println((*sort.Float64Slice).Less(&floatView, 1, 0))
	fmt.Println((*sort.Float64Slice).Search(&floatView, 0.25))
	(*sort.Float64Slice).Swap(&floatView, 0, 2)
	fmt.Println(floatView[0], floatView[1], floatView[2])
	(*sort.Float64Slice).Sort(&floatView)
	fmt.Println(floatView[0], floatView[1], floatView[2])

	strings := []string{"gamma", "alpha", "beta"}
	stringView := sort.StringSlice(strings)
	fmt.Println((*sort.StringSlice).Len(&stringView))
	fmt.Println((*sort.StringSlice).Less(&stringView, 1, 0))
	fmt.Println((*sort.StringSlice).Search(&stringView, "delta"))
	(*sort.StringSlice).Swap(&stringView, 0, 2)
	fmt.Println(stringView[0], stringView[1], stringView[2])
	(*sort.StringSlice).Sort(&stringView)
	fmt.Println(stringView[0], stringView[1], stringView[2])
}

func case_sort_string_slice_len() {
	values := []string{"gamma", "alpha", "beta"}
	fmt.Println(sort.StringSlice(values).Len())
}

func case_sort_string_slice_less() {
	values := []string{"gamma", "alpha"}
	fmt.Println(sort.StringSlice(values).Less(1, 0))
}

func case_sort_string_slice_search() {
	values := []string{"alpha", "delta", "omega"}
	fmt.Println(sort.StringSlice(values).Search("charlie"))
}

func case_sort_string_slice_sort() {
	values := []string{"gamma", "alpha", "beta"}
	sort.StringSlice(values).Sort()
	fmt.Println(values)
}

func case_sort_string_slice_swap() {
	values := []string{"alpha", "beta", "gamma"}
	sort.StringSlice(values).Swap(0, 2)
	fmt.Println(values)
}

func case_sort_strings() {
	values := []string{"pear", "apple", "banana"}
	sort.Strings(values)
	if values[0] == "apple" && values[2] == "pear" {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

func case_sort_strings_are_sorted() {
	values := []string{"apple", "banana", "pear"}
	if sort.StringsAreSorted(values) {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

func case_sort_basic_duplicates() {
	ints := []int{7, 2, 5, 2, 9}
	sort.Ints(ints)
	if sort.IntsAreSorted(ints) && ints[0] == 2 && ints[1] == 2 && ints[4] == 9 {
		fmt.Println("ints sorted")
	} else {
		fmt.Println("ints failed")
	}

	words := []string{"pear", "apple", "banana", "apple"}
	sort.Strings(words)
	if sort.StringsAreSorted(words) && words[0] == "apple" && words[3] == "pear" {
		fmt.Println("strings sorted")
	} else {
		fmt.Println("strings failed")
	}

	floats := []float64{3.5, -2.25, 0.5}
	sort.Float64s(floats)
	if sort.Float64sAreSorted(floats) && floats[0] == -2.25 && floats[2] == 3.5 {
		fmt.Println("floats sorted")
	} else {
		fmt.Println("floats failed")
	}
}
