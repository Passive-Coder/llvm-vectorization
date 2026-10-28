; ModuleID = '/Users/sreerajmuthaiya.a.l/llvm-vectorization/tests/fixtures/vectorizable.ll'
source_filename = "/Users/sreerajmuthaiya.a.l/llvm-vectorization/tests/fixtures/vectorizable.ll"
target datalayout = "e-p:64:64:64:64-i64:64-n8:16:32:64-S128"

define void @add_f32(ptr noalias %out, ptr noalias %left, ptr noalias %right, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds float, ptr %left, i64 %rv.index
  %rv.wide.load = load <4 x float>, ptr %rv.wide.ptr, align 4
  %rv.wide.ptr1 = getelementptr inbounds float, ptr %right, i64 %rv.index
  %rv.wide.load2 = load <4 x float>, ptr %rv.wide.ptr1, align 4
  %rv.binop = fadd <4 x float> %rv.wide.load, %rv.wide.load2
  %rv.wide.ptr3 = getelementptr inbounds float, ptr %out, i64 %rv.index
  store <4 x float> %rv.binop, ptr %rv.wide.ptr3, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %left.ptr = getelementptr inbounds float, ptr %left, i64 %i
  %left.value = load float, ptr %left.ptr, align 4
  %right.ptr = getelementptr inbounds float, ptr %right, i64 %i
  %right.value = load float, ptr %right.ptr, align 4
  %sum = fadd float %left.value, %right.value
  %out.ptr = getelementptr inbounds float, ptr %out, i64 %i
  store float %sum, ptr %out.ptr, align 4
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @scale_i32(ptr noalias %out, ptr noalias %input, i32 %factor, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i32, ptr %input, i64 %rv.index
  %rv.wide.load = load <4 x i32>, ptr %rv.wide.ptr, align 4
  %rv.splat.insert = insertelement <4 x i32> poison, i32 %factor, i32 0
  %rv.splat = shufflevector <4 x i32> %rv.splat.insert, <4 x i32> poison, <4 x i32> zeroinitializer
  %rv.binop = mul <4 x i32> %rv.wide.load, %rv.splat
  %rv.wide.ptr1 = getelementptr inbounds i32, ptr %out, i64 %rv.index
  store <4 x i32> %rv.binop, ptr %rv.wide.ptr1, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %input.ptr = getelementptr inbounds i32, ptr %input, i64 %i
  %input.value = load i32, ptr %input.ptr, align 4
  %product = mul i32 %input.value, %factor
  %out.ptr = getelementptr inbounds i32, ptr %out, i64 %i
  store i32 %product, ptr %out.ptr, align 4
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @write_index(ptr noalias %out, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 4
  %rv.vector.trip.count = and i64 %n, -2
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.splat.insert = insertelement <2 x i64> poison, i64 %rv.index, i32 0
  %rv.splat = shufflevector <2 x i64> %rv.splat.insert, <2 x i64> poison, <2 x i32> zeroinitializer
  %rv.lane.indices = add <2 x i64> %rv.splat, <i64 0, i64 1>
  %rv.wide.ptr = getelementptr inbounds i64, ptr %out, i64 %rv.index
  store <2 x i64> %rv.lane.indices, ptr %rv.wide.ptr, align 8
  %rv.index.next = add i64 %rv.index, 2
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %out.ptr = getelementptr inbounds i64, ptr %out, i64 %i
  store i64 %i, ptr %out.ptr, align 8
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @clamp_min_i32(ptr noalias %out, ptr noalias %input, i32 %minimum, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i32, ptr %input, i64 %rv.index
  %rv.wide.load = load <4 x i32>, ptr %rv.wide.ptr, align 4
  %rv.splat.insert = insertelement <4 x i32> poison, i32 %minimum, i32 0
  %rv.splat = shufflevector <4 x i32> %rv.splat.insert, <4 x i32> poison, <4 x i32> zeroinitializer
  %rv.icmp = icmp slt <4 x i32> %rv.wide.load, %rv.splat
  %rv.select = select <4 x i1> %rv.icmp, <4 x i32> %rv.splat, <4 x i32> %rv.wide.load
  %rv.wide.ptr1 = getelementptr inbounds i32, ptr %out, i64 %rv.index
  store <4 x i32> %rv.select, ptr %rv.wide.ptr1, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %input.ptr = getelementptr inbounds i32, ptr %input, i64 %i
  %input.value = load i32, ptr %input.ptr, align 4
  %too.small = icmp slt i32 %input.value, %minimum
  %clamped = select i1 %too.small, i32 %minimum, i32 %input.value
  %out.ptr = getelementptr inbounds i32, ptr %out, i64 %i
  store i32 %clamped, ptr %out.ptr, align 4
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @widen_i16(ptr noalias %out, ptr noalias %input, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i16, ptr %input, i64 %rv.index
  %rv.wide.load = load <4 x i16>, ptr %rv.wide.ptr, align 2
  %rv.cast = zext <4 x i16> %rv.wide.load to <4 x i32>
  %rv.binop = add <4 x i32> %rv.cast, splat (i32 7)
  %rv.wide.ptr1 = getelementptr inbounds i32, ptr %out, i64 %rv.index
  store <4 x i32> %rv.binop, ptr %rv.wide.ptr1, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %input.ptr = getelementptr inbounds i16, ptr %input, i64 %i
  %input.value = load i16, ptr %input.ptr, align 2
  %wide = zext i16 %input.value to i32
  %adjusted = add i32 %wide, 7
  %out.ptr = getelementptr inbounds i32, ptr %out, i64 %i
  store i32 %adjusted, ptr %out.ptr, align 4
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @increment_in_place(ptr noalias %data, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i32, ptr %data, i64 %rv.index
  %rv.wide.load = load <4 x i32>, ptr %rv.wide.ptr, align 4
  %rv.binop = add <4 x i32> %rv.wide.load, splat (i32 1)
  store <4 x i32> %rv.binop, ptr %rv.wide.ptr, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %element.ptr = getelementptr inbounds i32, ptr %data, i64 %i
  %old = load i32, ptr %element.ptr, align 4
  %new = add i32 %old, 1
  store i32 %new, ptr %element.ptr, align 4
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @increment_in_place_ne(ptr noalias %data, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i32, ptr %data, i64 %rv.index
  %rv.wide.load = load <4 x i32>, ptr %rv.wide.ptr, align 4
  %rv.binop = add <4 x i32> %rv.wide.load, splat (i32 2)
  store <4 x i32> %rv.binop, ptr %rv.wide.ptr, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %element.ptr = getelementptr inbounds i32, ptr %data, i64 %i
  %old = load i32, ptr %element.ptr, align 4
  %new = add i32 %old, 2
  store i32 %new, ptr %element.ptr, align 4
  %next = add nuw i64 %i, 1
  %continue = icmp ne i64 %next, %n
  br i1 %continue, label %loop, label %exit

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @increment_in_place_ult(ptr noalias %data, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i32, ptr %data, i64 %rv.index
  %rv.wide.load = load <4 x i32>, ptr %rv.wide.ptr, align 4
  %rv.binop = add <4 x i32> %rv.wide.load, splat (i32 3)
  store <4 x i32> %rv.binop, ptr %rv.wide.ptr, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %element.ptr = getelementptr inbounds i32, ptr %data, i64 %i
  %old = load i32, ptr %element.ptr, align 4
  %new = add i32 %old, 3
  store i32 %new, ptr %element.ptr, align 4
  %next = add nuw i64 %i, 1
  %continue = icmp ult i64 %next, %n
  br i1 %continue, label %loop, label %exit

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}

define void @ordered_double_store(ptr noalias %data, i64 %n) {
entry:
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %exit, label %rv.vector.dispatch

rv.vector.dispatch:                               ; preds = %entry
  %rv.enough.iterations = icmp uge i64 %n, 8
  %rv.vector.trip.count = and i64 %n, -4
  br i1 %rv.enough.iterations, label %rv.vector.body, label %loop

rv.vector.body:                                   ; preds = %rv.vector.body, %rv.vector.dispatch
  %rv.index = phi i64 [ 0, %rv.vector.dispatch ], [ %rv.index.next, %rv.vector.body ]
  %rv.wide.ptr = getelementptr inbounds i32, ptr %data, i64 %rv.index
  store <4 x i32> splat (i32 111), ptr %rv.wide.ptr, align 4
  store <4 x i32> splat (i32 222), ptr %rv.wide.ptr, align 4
  %rv.index.next = add i64 %rv.index, 4
  %rv.vector.done = icmp eq i64 %rv.index.next, %rv.vector.trip.count
  br i1 %rv.vector.done, label %rv.vector.exit, label %rv.vector.body

rv.vector.exit:                                   ; preds = %rv.vector.body
  %rv.has.remainder = icmp ne i64 %rv.vector.trip.count, %n
  br i1 %rv.has.remainder, label %loop, label %exit

loop:                                             ; preds = %rv.vector.exit, %rv.vector.dispatch, %loop
  %i = phi i64 [ 0, %rv.vector.dispatch ], [ %next, %loop ], [ %rv.vector.trip.count, %rv.vector.exit ]
  %element.ptr = getelementptr inbounds i32, ptr %data, i64 %i
  store i32 111, ptr %element.ptr, align 4
  store i32 222, ptr %element.ptr, align 4
  %next = add nuw i64 %i, 1
  %done = icmp eq i64 %next, %n
  br i1 %done, label %exit, label %loop

exit:                                             ; preds = %rv.vector.exit, %loop, %entry
  ret void
}
