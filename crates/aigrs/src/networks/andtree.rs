use std::ops::Not;

#[derive(Debug, Clone, Copy)]
pub struct False(bool);

impl std::ops::Not for False {
    type Output = False;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}


#[derive(Debug, Clone, Copy)]
pub struct And<A, B> {
    pub inv: bool,
    pub lhs: A,
    pub rhs: B,
}

impl <TL, TR> And<TL, TR> {
    pub fn andx(lhs: TL, rhs: TR, inv: bool) -> Self {
        Self { inv, lhs, rhs }
    }
    
    pub fn and(lhs: TL, rhs: TR) -> Self {
        Self::andx(lhs, rhs, false)
    }

    pub fn or<A, B>(lhs: A, rhs: B) -> And<A, B>
        where A: Not<Output=A>, B: Not<Output=B>
    {
        !And::and(!lhs, !rhs)
    }
}

impl <A, B> std::ops::Not for And<A, B> {
    type Output = Self;

    fn not(mut self) -> Self::Output {
        self.inv = !self.inv;
        self
    }
}

impl <A, B, C> std::ops::BitAnd<C> for And<A, B> {
    type Output = And<And<A, B>, C>;

    fn bitand(self, rhs: C) -> Self::Output {
        And {
            inv: false,
            lhs: self,
            rhs,
        }
    }
}

impl <A, B, C: std::ops::Not<Output = C>> std::ops::BitOr<C> for And<A, B> {
    type Output = And<And<A, B>, C>;

    fn bitor(self, rhs: C) -> Self::Output {
        !(!self & !rhs)
    }
}

impl <A, B, C, D> std::ops::BitXor<And<C, D>> for And<A, B>
    where A: Copy, B: Copy, C: Copy, D: Copy
{
    type Output = And<And<And<A, B>, And<C, D>>, And<And<A, B>, And<C, D>>>;

    fn bitxor(self, rhs: And<C, D>) -> Self::Output {
        (self & !rhs) | (!self & rhs)
    }
}
