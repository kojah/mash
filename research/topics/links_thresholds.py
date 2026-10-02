"""Links equations 16/24 only; not a graph clusterer or selected topic method."""
from dataclasses import dataclass
import math


@dataclass(frozen=True)
class Parameters:
    tc: float=.5
    ts: float=.8
    tp: float=1.

    def __post_init__(self):
        if (not all(math.isfinite(v) for v in (self.tc,self.ts,self.tp))
                or not 0<self.tc<1 or not 0<self.ts<1 or not self.tc*self.tc<=self.tp<=1):
            raise ValueError('finite Links threshold domain required')

    def pair(self,k,other=1):
        if type(k) is not int or type(other) is not int or k<1 or other<1:
            raise ValueError('positive integer subcluster sizes required')
        tc2=self.tc*self.tc;alpha=1/tc2-1
        raw=1/math.sqrt((1+alpha/k)*(1+alpha/other))
        return tc2+(self.tp-tc2)/(1-tc2)*(raw-tc2)
