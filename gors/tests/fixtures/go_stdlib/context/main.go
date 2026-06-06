package main

import (
	"context"
	"fmt"
	"time"
)

type contextKey string

type causeError string

func (e causeError) Error() string {
	return string(e)
}

func main() {
	// gors:stdlib-cover context::Canceled
	fmt.Println(context.Canceled.Error())
	// gors:stdlib-cover context::DeadlineExceeded
	fmt.Println(context.DeadlineExceeded.Error())

	// gors:stdlib-cover context::Background
	// gors:stdlib-cover context::Context
	bg := context.Background()
	_, bgDeadline := bg.Deadline()
	fmt.Println("background", bgDeadline, bg.Done() == nil, bg.Err() == nil, bg.Value("k") == nil)

	// gors:stdlib-cover context::TODO
	todo := context.TODO()
	_, todoDeadline := todo.Deadline()
	fmt.Println("todo", todoDeadline, todo.Done() == nil, todo.Err() == nil, todo.Value("k") == nil)

	// gors:stdlib-cover context::WithValue
	{
		valued := context.WithValue(bg, contextKey("name"), "ada")
		fmt.Println("with-value", valued.Value(contextKey("name")), valued.Value(contextKey("missing")) == nil)
	}

	// gors:stdlib-cover context::WithoutCancel
	{
		parent, parentCancel := context.WithCancelCause(context.WithValue(bg, contextKey("kept"), "value"))
		detached := context.WithoutCancel(parent)
		_, detachedDeadline := detached.Deadline()
		fmt.Println("without-cancel-before", detachedDeadline, detached.Done() == nil, detached.Err() == nil, context.Cause(detached) == nil, detached.Value(contextKey("kept")))
		parentCancel(causeError("parent canceled"))
		fmt.Println("without-cancel-after", detached.Done() == nil, detached.Err() == nil, context.Cause(detached) == nil, detached.Value(contextKey("kept")), parent.Err().Error(), context.Cause(parent).Error())
	}

	// gors:stdlib-cover context::WithCancel
	// gors:stdlib-cover context::CancelFunc
	cancelable, cancel := context.WithCancel(bg)
	_, cancelDeadline := cancelable.Deadline()
	fmt.Println("with-cancel-before", cancelDeadline, cancelable.Err() == nil, cancelable.Value(contextKey("name")) == nil)
	select {
	case <-cancelable.Done():
		fmt.Println("with-cancel-open", false)
	default:
		fmt.Println("with-cancel-open", true)
	}
	cancel()
	fmt.Println("with-cancel-after", cancelable.Err().Error())
	select {
	case <-cancelable.Done():
		fmt.Println("with-cancel-closed", true)
	default:
		fmt.Println("with-cancel-closed", false)
	}
	cancel()
	fmt.Println("with-cancel-idempotent", cancelable.Err().Error())

	// gors:stdlib-cover context::AfterFunc
	{
		stoppedCtx, stoppedCancel := context.WithCancel(bg)
		stoppedDone := make(chan struct{})
		stoppedStop := context.AfterFunc(stoppedCtx, func() {
			close(stoppedDone)
		})
		fmt.Println("afterfunc-stop-before-cancel", stoppedStop())
		stoppedCancel()
		select {
		case <-stoppedDone:
			fmt.Println("afterfunc-stopped-ran", true)
		default:
			fmt.Println("afterfunc-stopped-ran", false)
		}

		runCtx, runCancel := context.WithCancel(bg)
		runDone := make(chan struct{})
		runStop := context.AfterFunc(runCtx, func() {
			close(runDone)
		})
		runCancel()
		<-runDone
		fmt.Println("afterfunc-ran", runCtx.Err().Error(), runStop())
	}

	// gors:stdlib-cover context::CancelCauseFunc
	// gors:stdlib-cover context::Cause
	// gors:stdlib-cover context::WithCancelCause
	{
		causeCtx, causeCancel := context.WithCancelCause(bg)
		fmt.Println("with-cancel-cause-before", causeCtx.Err() == nil, context.Cause(causeCtx) == nil)
		causeCancel(causeError("manual cause"))
		fmt.Println("with-cancel-cause-after", causeCtx.Err().Error(), context.Cause(causeCtx).Error())
		causeCancel(causeError("ignored cause"))
		fmt.Println("with-cancel-cause-idempotent", context.Cause(causeCtx).Error())

		nilCtx, nilCancel := context.WithCancelCause(bg)
		nilCancel(nil)
		fmt.Println("with-cancel-cause-nil", nilCtx.Err().Error(), context.Cause(nilCtx).Error())

		parentFirst, cancelParentFirst := context.WithCancelCause(bg)
		childAfterParent, cancelChildAfterParent := context.WithCancelCause(parentFirst)
		cancelParentFirst(causeError("parent cause"))
		cancelChildAfterParent(causeError("child cause"))
		fmt.Println("with-cancel-cause-parent-first", context.Cause(parentFirst).Error(), context.Cause(childAfterParent).Error())

		parentAfterChild, cancelParentAfterChild := context.WithCancelCause(bg)
		childFirst, cancelChildFirst := context.WithCancelCause(parentAfterChild)
		cancelChildFirst(causeError("child cause"))
		cancelParentAfterChild(causeError("parent cause"))
		fmt.Println("with-cancel-cause-child-first", context.Cause(parentAfterChild).Error(), context.Cause(childFirst).Error())
	}

	// gors:stdlib-cover context::WithDeadline
	// gors:stdlib-cover context::WithDeadlineCause
	// gors:stdlib-cover context::WithTimeout
	// gors:stdlib-cover context::WithTimeoutCause
	{
		past := time.Now().Add(-time.Second)
		deadlineCtx, deadlineCancel := context.WithDeadline(bg, past)
		deadline, deadlineOK := deadlineCtx.Deadline()
		fmt.Println("with-deadline-deadline", deadlineOK, deadline.Equal(past))
		fmt.Println("with-deadline-expired", deadlineCtx.Err().Error(), context.Cause(deadlineCtx).Error())
		select {
		case <-deadlineCtx.Done():
			fmt.Println("with-deadline-closed", true)
		default:
			fmt.Println("with-deadline-closed", false)
		}
		deadlineCancel()
		fmt.Println("with-deadline-idempotent", deadlineCtx.Err().Error())

		deadlineCauseCtx, deadlineCauseCancel := context.WithDeadlineCause(bg, past, causeError("deadline cause"))
		fmt.Println("with-deadline-cause", deadlineCauseCtx.Err().Error(), context.Cause(deadlineCauseCtx).Error())
		deadlineCauseCancel()

		timeoutCtx, timeoutCancel := context.WithTimeout(bg, -time.Second)
		fmt.Println("with-timeout", timeoutCtx.Err().Error(), context.Cause(timeoutCtx).Error())
		timeoutCancel()

		timeoutCauseCtx, timeoutCauseCancel := context.WithTimeoutCause(bg, -time.Second, causeError("timeout cause"))
		fmt.Println("with-timeout-cause", timeoutCauseCtx.Err().Error(), context.Cause(timeoutCauseCtx).Error())
		timeoutCauseCancel()
	}
}
