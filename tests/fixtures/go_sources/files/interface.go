package main

type i1 interface{}

type i2 interface {
	Read(byte) (int, error)
	Write(byte) (int, error)
	Close() error
}

type i3 interface {
	i2
}

type Locker interface {
	Lock()
	Unlock()
}
