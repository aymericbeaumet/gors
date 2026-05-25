package main

import (
	"fmt"
	"sort"
)

func main() {
	fmt.Println("== sort/find ==")
	case_sort_find()
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
	sort.Slice(values, case_sort_slice_keepOrder)
	fmt.Println(values)
}

func case_sort_slice_is_sorted_keepOrder(i int, j int) bool {
	return i < j
}

func case_sort_slice_is_sorted() {
	values := []int{3, 2, 1}
	fmt.Println(sort.SliceIsSorted(values, case_sort_slice_is_sorted_keepOrder))
}

func case_sort_slice_stable_keepOrder(i int, j int) bool {
	return i < j
}

func case_sort_slice_stable() {
	values := []string{"a", "b", "c"}
	sort.SliceStable(values, case_sort_slice_stable_keepOrder)
	fmt.Println(values)
}

func case_sort_sort() {
	values := []int{3, 1, 2}
	sort.Sort(sort.IntSlice(values))
	fmt.Println(values)
}

func case_sort_stable() {
	values := []string{"gamma", "alpha", "beta"}
	sort.Stable(sort.StringSlice(values))
	fmt.Println(values)
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
