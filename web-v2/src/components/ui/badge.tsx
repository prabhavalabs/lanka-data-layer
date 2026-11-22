import { cva, type VariantProps } from 'class-variance-authority'
import type * as React from 'react'
import { cn } from '@/lib/utils'

const badgeVariants = cva('inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11.5px] font-medium leading-5 whitespace-nowrap [&_svg]:size-3', {
  variants: {
    variant: {
      neutral: 'border-line2 bg-bg3 text-ink2',
      outline: 'border-line2 text-ink2',
      brand: 'border-transparent bg-brand-soft text-brand-ink',
      ok: 'border-transparent bg-ok-soft text-ok',
      warn: 'border-transparent bg-warn-soft text-warn',
      bad: 'border-transparent bg-bad-soft text-bad',
      mono: 'rounded-md border-line2 bg-bg3 font-mono text-[11px] text-ink2',
    },
  },
  defaultVariants: { variant: 'neutral' },
})

function Badge({ className, variant, ...props }: React.ComponentProps<'span'> & VariantProps<typeof badgeVariants>) {
  return <span data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />
}
export { Badge, badgeVariants }
