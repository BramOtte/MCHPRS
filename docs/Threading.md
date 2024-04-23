# Threading Backend

The new backend implements threading by separating the node graph multiple groups.
Here a group means a set of transitively connected redstone components.

You can activate it by passing the `--threading` or `-T` flag to the redpiler.

To seperate a group into multiple smaller ones you must place a separator in between all redstone lines connecting them.
A separator is a comparator with an annotation sign next to it. The sign must have "[separate #]" written on it, where # is the amount of delay.
Optimal delay values are not known yet, but they should probably be in the range of 5-100. Lower values will result in lower latency, but higher values should improve peak performance.

## Example

Here a CPU is connected to a screen by three redstone lines. To separate these two, place signs with the separate annotation between all lines connecting them.
(Dots are redstone dust; ">" and "<" are the comparators that need the annotation)
```
-----+         +----------+
     | ...>... |          |
     |         |          |
 CPU | ...>... |  Screen  |
     |         |          |
     | ...<... |          |
-----+         +----------+
```

Note: Remember to rotate the signs correctly, they annotate the block right behind them, or alternatively the one above/below that.
