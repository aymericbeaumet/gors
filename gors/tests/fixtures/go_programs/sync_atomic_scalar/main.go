package main

import "sync/atomic"

type state struct {
	word uint64
}

func expect(ok bool) {
	if !ok {
		panic("unexpected atomic result")
	}
}

func checkInt32() int32 {
	var value int32 = 1
	expect(atomic.SwapInt32(&value, 3) == 1)
	expect(atomic.CompareAndSwapInt32(&value, 3, 5))
	expect(!atomic.CompareAndSwapInt32(&value, 3, 9))
	expect(atomic.AddInt32(&value, 2) == 7)
	expect(atomic.AndInt32(&value, 6) == 7)
	expect(atomic.OrInt32(&value, 1) == 6)
	expect(atomic.LoadInt32(&value) == 7)
	atomic.StoreInt32(&value, 11)
	expect(atomic.LoadInt32(&value) == 11)
	atomic.StoreInt32(&value, 2147483647)
	expect(atomic.AddInt32(&value, 1) == -2147483648)
	return value
}

func checkInt64() int64 {
	var value int64 = 1
	expect(atomic.SwapInt64(&value, 3) == 1)
	expect(atomic.CompareAndSwapInt64(&value, 3, 5))
	expect(!atomic.CompareAndSwapInt64(&value, 3, 9))
	expect(atomic.AddInt64(&value, 2) == 7)
	expect(atomic.AndInt64(&value, 6) == 7)
	expect(atomic.OrInt64(&value, 1) == 6)
	expect(atomic.LoadInt64(&value) == 7)
	atomic.StoreInt64(&value, 11)
	expect(atomic.LoadInt64(&value) == 11)
	return value
}

func checkUint32() uint32 {
	var value uint32 = 1
	expect(atomic.SwapUint32(&value, 3) == 1)
	expect(atomic.CompareAndSwapUint32(&value, 3, 5))
	expect(!atomic.CompareAndSwapUint32(&value, 3, 9))
	expect(atomic.AddUint32(&value, 2) == 7)
	expect(atomic.AndUint32(&value, 6) == 7)
	expect(atomic.OrUint32(&value, 1) == 6)
	expect(atomic.LoadUint32(&value) == 7)
	atomic.StoreUint32(&value, 11)
	expect(atomic.LoadUint32(&value) == 11)
	return value
}

func checkUint64Loop() uint64 {
	value := &state{word: 2}
	for {
		old := atomic.LoadUint64(&value.word)
		if old&1 != 0 {
			continue
		}
		if atomic.CompareAndSwapUint64(&value.word, old, old|1) {
			break
		}
	}
	expect(atomic.SwapUint64(&value.word, 3) == 3)
	expect(atomic.CompareAndSwapUint64(&value.word, 3, 5))
	expect(!atomic.CompareAndSwapUint64(&value.word, 3, 9))
	expect(atomic.AddUint64(&value.word, 2) == 7)
	expect(atomic.AndUint64(&value.word, 6) == 7)
	expect(atomic.OrUint64(&value.word, 1) == 6)
	expect(atomic.LoadUint64(&value.word) == 7)
	atomic.StoreUint64(&value.word, 11)
	expect(atomic.LoadUint64(&value.word) == 11)
	return value.word
}

func checkUintptr() uintptr {
	var value uintptr = 1
	expect(atomic.SwapUintptr(&value, 3) == 1)
	expect(atomic.CompareAndSwapUintptr(&value, 3, 5))
	expect(!atomic.CompareAndSwapUintptr(&value, 3, 9))
	expect(atomic.AddUintptr(&value, 2) == 7)
	expect(atomic.AndUintptr(&value, 6) == 7)
	expect(atomic.OrUintptr(&value, 1) == 6)
	expect(atomic.LoadUintptr(&value) == 7)
	atomic.StoreUintptr(&value, 11)
	expect(atomic.LoadUintptr(&value) == 11)
	return value
}

func main() {
	expect(checkInt32() == -2147483648)
	expect(checkInt64() == 11)
	expect(checkUint32() == 11)
	expect(checkUint64Loop() == 11)
	expect(checkUintptr() == 11)
}
