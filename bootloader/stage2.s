	.file	"stage2.c"
	.code16gcc
	.text
	.def	_puts;	.scl	3;	.type	32;	.endef
_puts:
	movl	%eax, %edx
	movzbl	(%eax), %eax
	testb	%al, %al
	jne	L6
	ret
L6:
	pushl	%ebp
	movl	%esp, %ebp
	pushl	%ebx
L3:
	incl	%edx
	orb	$14, %ah
	movl	$7, %ebx
/APP
 # 73 "stage2/stage2.c" 1
	int $0x10
 # 0 "" 2
/NO_APP
	movzbl	(%edx), %eax
	testb	%al, %al
	jne	L3
	popl	%ebx
	popl	%ebp
	ret
	.def	_puthex;	.scl	3;	.type	32;	.endef
_puthex:
	pushl	%ebp
	movl	%esp, %ebp
	pushl	%edi
	pushl	%esi
	pushl	%ebx
	movl	%eax, %edx
	movl	$7, %ebx
	movl	$3632, %eax
/APP
 # 73 "stage2/stage2.c" 1
	int $0x10
 # 0 "" 2
/NO_APP
	movl	$3704, %eax
/APP
 # 73 "stage2/stage2.c" 1
	int $0x10
 # 0 "" 2
/NO_APP
	movl	$28, %ecx
L11:
	movl	%edx, %esi
	shrl	%cl, %esi
	andl	$15, %esi
	leal	55(%esi), %eax
	movl	%esi, %ebx
	cmpb	$9, %bl
	ja	L10
	leal	48(%esi), %eax
L10:
	movzbl	%al, %eax
	orb	$14, %ah
	movl	$7, %ebx
/APP
 # 73 "stage2/stage2.c" 1
	int $0x10
 # 0 "" 2
/NO_APP
	subl	$4, %ecx
	cmpl	$-4, %ecx
	jne	L11
	popl	%ebx
	popl	%esi
	popl	%edi
	popl	%ebp
	ret
	.def	_read_sectors;	.scl	3;	.type	32;	.endef
_read_sectors:
	pushl	%ebp
	movl	%esp, %ebp
	pushl	%esi
	subl	$16, %esp
	movw	$16, -20(%ebp)
	movw	%cx, -18(%ebp)
	movl	%edx, %ecx
	andl	$15, %ecx
	movw	%cx, -16(%ebp)
	shrl	$4, %edx
	movw	%dx, -14(%ebp)
	movl	%eax, -12(%ebp)
	xorl	%eax, %eax
	movl	%eax, -8(%ebp)
	movb	32252, %dl
	leal	-20(%ebp), %ecx
/APP
 # 110 "stage2/stage2.c" 1
	movw %cx, %si
movw $0x4200, %ax
int $0x13
setnc %dl

 # 0 "" 2
/NO_APP
	movzbl	%dl, %eax
	addl	$16, %esp
	popl	%esi
	popl	%ebp
	ret
	.def	_next_cluster;	.scl	3;	.type	32;	.endef
_next_cluster:
	pushl	%ebp
	movl	%esp, %ebp
	pushl	%ebx
	movzwl	14(%eax), %ecx
	cmpw	$0, 22(%eax)
	je	L17
	leal	(%edx,%edx), %ebx
	movl	%ebx, %eax
	shrl	$9, %eax
	addl	%ecx, %eax
	movl	$1, %ecx
	movl	$1536, %edx
	call	_read_sectors
	testl	%eax, %eax
	je	L26
	andl	$510, %ebx
	movzwl	1536(%ebx), %eax
	cmpw	$-9, %ax
	jmp	L27
L17:
	leal	0(,%edx,4), %ebx
	movl	%ebx, %eax
	shrl	$9, %eax
	addl	%ecx, %eax
	movl	$1, %ecx
	movl	$1536, %edx
	call	_read_sectors
	testl	%eax, %eax
	je	L26
	andl	$508, %ebx
	movl	1536(%ebx), %eax
	andl	$268435455, %eax
	cmpl	$268435447, %eax
L27:
	jbe	L16
L26:
	xorl	%eax, %eax
L16:
	popl	%ebx
	popl	%ebp
	ret
	.section .rdata,"dr"
LC0:
	.ascii "FAT16\0"
LC1:
	.ascii "FAT32\0"
LC2:
	.ascii "=== SKYOS Bootloader v0.1 ===\12\12\0"
LC3:
	.ascii "BPB read fail\12\0"
LC4:
	.ascii "FS: \0"
LC5:
	.ascii "\12\0"
LC6:
	.ascii "\12[K] Boot kernel      [R] Reboot\12\0"
LC7:
	.ascii "boot> \0"
LC8:
	.ascii "?\12\0"
LC9:
	.ascii "\12Searching KERNEL.BIN ...\12\0"
LC10:
	.ascii "kernel.bin\0"
LC11:
	.ascii "Not found\12\0"
LC12:
	.ascii "cluster=\0"
LC13:
	.ascii "  size=\0"
LC14:
	.ascii "Load fail\12\0"
LC15:
	.ascii "Loaded. Jumping ...\12\0"
LC16:
	.ascii "HALT\12\0"
	.text
	.globl	__start
	.def	__start;	.scl	2;	.type	32;	.endef
__start:
	pushl	%ebp
	movl	%esp, %ebp
	pushl	%edi
	pushl	%esi
	pushl	%ebx
	andl	$-16, %esp
	subl	$96, %esp
/APP
 # 85 "stage2/stage2.c" 1
	mov $0x0600, %ax; xor %cx, %cx; mov $0x184F, %dx; mov $0x07, %bh; int $0x10
 # 0 "" 2
 # 88 "stage2/stage2.c" 1
	mov $0x0200, %ax; xor %dx, %dx; xor %bx, %bx; int $0x10
 # 0 "" 2
/NO_APP
	movl	$LC2, %eax
	call	_puts
	leal	44(%esp), %edx
	movl	$1, %ecx
	xorl	%eax, %eax
	call	_read_sectors
	testl	%eax, %eax
	jne	L29
	movl	$LC3, %eax
	jmp	L104
L29:
	movl	$LC4, %eax
	call	_puts
	movzwl	66(%esp), %ecx
	movl	$LC1, %eax
	testw	%cx, %cx
	je	L31
	movl	$LC0, %eax
L31:
	call	_puts
	movl	$LC5, %eax
	call	_puts
	movl	$LC6, %eax
L103:
	call	_puts
	movl	$LC7, %eax
	call	_puts
/APP
 # 95 "stage2/stage2.c" 1
	int $0x16
 # 0 "" 2
/NO_APP
	movl	%eax, %edx
	movzbl	%al, %eax
	orb	$14, %ah
	movl	$7, %ebx
/APP
 # 73 "stage2/stage2.c" 1
	int $0x10
 # 0 "" 2
/NO_APP
	movl	$3594, %eax
/APP
 # 73 "stage2/stage2.c" 1
	int $0x10
 # 0 "" 2
/NO_APP
	andl	$-33, %edx
	cmpb	$75, %dl
	je	L32
	cmpb	$82, %dl
	jne	L33
/APP
 # 266 "stage2/stage2.c" 1
	int $0x19
 # 0 "" 2
/NO_APP
L33:
	movl	$LC8, %eax
	jmp	L103
L32:
	movl	$LC9, %eax
	call	_puts
	xorl	%eax, %eax
L35:
	movb	$32, 33(%esp,%eax)
	incl	%eax
	cmpl	$11, %eax
	jne	L35
	xorl	%edx, %edx
L36:
	movb	LC10(%edx), %al
	testb	%al, %al
	je	L38
	cmpb	$46, %al
	je	L74
	cmpl	$8, %edx
	je	L38
	leal	-97(%eax), %ebx
	cmpb	$25, %bl
	ja	L37
	subl	$32, %eax
L37:
	movb	%al, 33(%esp,%edx)
	incl	%edx
	jmp	L36
L105:
	cmpl	$11, %ebx
	je	L38
	movl	%eax, %edi
	leal	-97(%edi), %eax
	cmpb	$25, %al
	ja	L43
	subl	$32, %edi
L43:
	movl	%edi, %eax
	movb	%al, 33(%esp,%ebx)
	incl	%ebx
L39:
	movb	LC10-7(%edx,%ebx), %al
	testb	%al, %al
	jne	L105
L38:
	testw	%cx, %cx
	je	L106
	movzbl	60(%esp), %edi
	imull	%ecx, %edi
	movzwl	58(%esp), %eax
	addl	%eax, %edi
	movzwl	61(%esp), %eax
	sall	$5, %eax
	addl	$511, %eax
	shrl	$9, %eax
	sall	$4, %eax
	movl	%eax, 28(%esp)
	xorl	%esi, %esi
	jmp	L46
L74:
	movl	$8, %ebx
	jmp	L39
L106:
	movl	88(%esp), %ebx
	movzbl	57(%esp), %eax
	sall	$4, %eax
	movl	%eax, 28(%esp)
	jmp	L47
L101:
	movb	1536(%ebx), %al
	cmpb	$-27, %al
	je	L52
	xorl	%eax, %eax
L53:
	movb	(%edx,%eax), %cl
	cmpb	33(%esp,%eax), %cl
	je	L107
L52:
	incl	%esi
L46:
	cmpl	28(%esp), %esi
	jge	L49
	movl	%esi, %ebx
	sall	$5, %ebx
	andl	$511, %ebx
	je	L48
L51:
	leal	1536(%ebx), %edx
	movb	1536(%ebx), %al
	testb	%al, %al
	jne	L101
L49:
	movl	$LC11, %eax
L104:
	call	_puts
	jmp	L30
L48:
	movl	%esi, %eax
	sarl	$4, %eax
	addl	%edi, %eax
	movl	$1, %ecx
	movl	$1536, %edx
	call	_read_sectors
	testl	%eax, %eax
	jne	L51
	jmp	L49
L107:
	incl	%eax
	cmpl	$11, %eax
	jne	L53
	movzwl	26(%edx), %ebx
	movl	28(%edx), %esi
	jmp	L54
L62:
	movzbl	57(%esp), %edi
	movzwl	58(%esp), %edx
	movzbl	60(%esp), %esi
	movzwl	66(%esp), %ecx
	testw	%cx, %cx
	jne	L57
	movl	80(%esp), %ecx
L57:
	movzwl	61(%esp), %eax
	sall	$5, %eax
	addl	$511, %eax
	shrl	$9, %eax
	addl	%edx, %eax
	leal	-2(%ebx), %edx
	imull	%edi, %edx
	addl	%edx, %eax
	imull	%ecx, %esi
	addl	%esi, %eax
	movl	%edi, %ecx
	movl	$1536, %edx
	call	_read_sectors
	testl	%eax, %eax
	je	L49
	xorl	%edi, %edi
L58:
	cmpl	%edi, 28(%esp)
	jg	L61
	leal	44(%esp), %eax
	movl	%ebx, %edx
	call	_next_cluster
	movl	%eax, %ebx
L47:
	testl	%ebx, %ebx
	jne	L62
	jmp	L49
L61:
	movl	%edi, %eax
	sall	$5, %eax
	leal	1536(%eax), %ecx
	movb	1536(%eax), %dl
	testb	%dl, %dl
	je	L49
	movb	1536(%eax), %al
	cmpb	$-27, %al
	je	L59
	xorl	%eax, %eax
L60:
	movb	(%ecx,%eax), %dl
	cmpb	33(%esp,%eax), %dl
	jne	L59
	incl	%eax
	cmpl	$11, %eax
	jne	L60
	movw	20(%ecx), %bx
	movzwl	26(%ecx), %eax
	sall	$16, %ebx
	orl	%eax, %ebx
	movl	28(%ecx), %esi
	jmp	L54
L59:
	incl	%edi
	jmp	L58
L54:
	movl	$LC12, %eax
	call	_puts
	movl	%ebx, %eax
	call	_puthex
	movl	$LC13, %eax
	call	_puts
	movl	%esi, %eax
	call	_puthex
	movl	$LC5, %eax
	call	_puts
	movl	$65536, 28(%esp)
L63:
	testl	%ebx, %ebx
	je	L68
	testl	%esi, %esi
	je	L70
	movzbl	57(%esp), %eax
	movl	%eax, 24(%esp)
	movl	%eax, %edi
	sall	$9, %edi
	cmpl	%edi, %esi
	jnb	L64
	movl	%esi, %edi
L64:
	leal	511(%edi), %eax
	shrl	$9, %eax
	movl	%eax, 20(%esp)
	movzwl	58(%esp), %ecx
	movzbl	60(%esp), %eax
	movl	%eax, 16(%esp)
	movzwl	66(%esp), %eax
	testw	%ax, %ax
	jne	L66
	movl	80(%esp), %eax
L66:
	movzwl	61(%esp), %edx
	sall	$5, %edx
	addl	$511, %edx
	shrl	$9, %edx
	addl	%edx, %ecx
	movl	%ecx, 12(%esp)
	leal	-2(%ebx), %ecx
	imull	24(%esp), %ecx
	movl	12(%esp), %edx
	addl	%ecx, %edx
	movl	16(%esp), %ecx
	imull	%eax, %ecx
	leal	(%ecx,%edx), %eax
	movl	20(%esp), %ecx
	movl	28(%esp), %edx
	call	_read_sectors
	testl	%eax, %eax
	jne	L67
L71:
	movl	$LC14, %eax
	jmp	L104
L67:
	addl	%edi, 28(%esp)
	subl	%edi, %esi
	leal	44(%esp), %eax
	movl	%ebx, %edx
	call	_next_cluster
	movl	%eax, %ebx
	jmp	L63
L68:
	testl	%esi, %esi
	jne	L71
L70:
	movl	$LC15, %eax
	call	_puts
/APP
 # 283 "stage2/stage2.c" 1
	ljmp $4096, $0
 # 0 "" 2
/NO_APP
L30:
	movl	$LC16, %eax
	call	_puts
L72:
/APP
 # 287 "stage2/stage2.c" 1
	hlt
 # 0 "" 2
/NO_APP
	jmp	L72
	.ident	"GCC: (GNU) 15.2.0"
