import { Slider as SliderPrimitive } from 'radix-ui'
import type * as React from 'react'
import { cn } from '@/lib/utils'

function Slider({ className, defaultValue, value, 'aria-label': ariaLabel, ...props }: React.ComponentProps<typeof SliderPrimitive.Root>) {
  const thumbs = (value ?? defaultValue ?? [0]).length
  return (
    <SliderPrimitive.Root
      data-slot="slider"
      defaultValue={defaultValue}
      value={value}
      className={cn('relative flex h-5 w-full touch-none items-center select-none data-[disabled]:opacity-50', className)}
      {...props}
    >
      <SliderPrimitive.Track className="relative h-[3px] grow overflow-hidden rounded-full bg-line2">
        <SliderPrimitive.Range className="absolute h-full bg-ink" />
      </SliderPrimitive.Track>
      {Array.from({ length: thumbs }, (_, i) => (
        <SliderPrimitive.Thumb
          key={i}
          aria-label={thumbs > 1 ? `${ariaLabel ?? 'Value'} ${i === 0 ? 'minimum' : 'maximum'}` : ariaLabel}
          className="block size-[15px] rounded-full border-[1.5px] border-ink bg-bg2 shadow-[0_1px_4px_rgba(0,0,0,0.35)] outline-none transition-transform duration-150 ease-out focus-visible:ring-4 focus-visible:ring-brand/30 active:scale-110"
        />
      ))}
    </SliderPrimitive.Root>
  )
}
export { Slider }
