import { Slot } from 'radix-ui'
import { cva, type VariantProps } from 'class-variance-authority'
import type * as React from 'react'
import { cn } from '@/lib/utils'

const buttonVariants = cva(
  "pressable inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap font-medium outline-none disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        primary:
          'bg-ink text-bg shadow-[inset_0_1px_0_rgba(255,255,255,0.25)] [@media(hover:hover)_and_(pointer:fine)]:hover:bg-ink/88',
        brand:
          'bg-brand text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.22),0_6px_18px_-6px_var(--brand)] [@media(hover:hover)_and_(pointer:fine)]:hover:brightness-110',
        outline:
          'border border-line2 bg-bg2/60 text-ink [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/60',
        glass:
          'glass text-ink [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3/70',
        ghost: 'text-ink2 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink',
        link: 'text-brand-ink underline-offset-4 [@media(hover:hover)_and_(pointer:fine)]:hover:underline',
      },
      size: {
        sm: 'h-8 rounded-md px-3 text-[13px]',
        md: 'h-9 rounded-lg px-3.5 text-sm',
        lg: 'h-11 rounded-[11px] px-5 text-[15px]',
        icon: 'size-9 rounded-lg',
        'icon-sm': 'size-8 rounded-md',
      },
    },
    defaultVariants: { variant: 'primary', size: 'md' },
  },
)

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<'button'> & VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : 'button'
  return <Comp data-slot="button" className={cn(buttonVariants({ variant, size }), className)} {...props} />
}

export { Button, buttonVariants }
