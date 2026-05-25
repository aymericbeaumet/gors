package main

import (
	"fmt"
	"sort"
)

func reverseIndex(i int, j int) bool {
	return i > j
}

func keepIndex(i int, j int) bool {
	return i < j
}

func main() {
	ints := []int{5, 2, 9, 2}
	sort.Ints(ints)
	fmt.Println(ints, sort.IntsAreSorted(ints), sort.SearchInts(ints, 5))

	words := []string{"pear", "apple", "banana"}
	sort.Strings(words)
	fmt.Println(words, sort.StringsAreSorted(words), sort.SearchStrings(words, "pear"))

	floats := []float64{3.5, -1.25, 0.5}
	sort.Float64s(floats)
	fmt.Println(floats, sort.Float64sAreSorted(floats), sort.SearchFloat64s(floats, 0.5))

	fmt.Println(sort.Search(10, func(i int) bool { return i*i >= 30 }))
	idx, found := sort.Find(10, func(i int) int { return 6 - i })
	fmt.Println(idx, found)

	pairs := []int{1, 2, 3, 4}
	sort.Slice(pairs, reverseIndex)
	fmt.Println(pairs, sort.SliceIsSorted(pairs, reverseIndex))

	stable := []int{3, 1, 2}
	sort.SliceStable(stable, keepIndex)
	fmt.Println(stable)

	sort.Sort(sort.Reverse(sort.IntSlice(ints)))
	fmt.Println(ints)

	fmt.Println(sort.IntSlice(ints).Len(), sort.IntSlice(ints).Less(0, 1), sort.IntSlice(ints).Search(5))
	sort.IntSlice(ints).Swap(0, 1)
	fmt.Println(ints)

	sort.StringSlice(words).Sort()
	fmt.Println(sort.StringSlice(words).Len(), sort.StringSlice(words).Less(0, 1), sort.StringSlice(words).Search("pear"))
	sort.StringSlice(words).Swap(0, 1)
	fmt.Println(words)

	sort.Float64Slice(floats).Sort()
	fmt.Println(sort.Float64Slice(floats).Len(), sort.Float64Slice(floats).Less(0, 1), sort.Float64Slice(floats).Search(0.5))
	sort.Float64Slice(floats).Swap(0, 1)
	fmt.Println(floats)
}
