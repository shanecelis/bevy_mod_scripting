use ::std::borrow::Cow;

use ::std::marker::PhantomData;


use ::std::convert::AsRef;

use crate::{LuaProxyable,ToLuaProxy,ScriptRef, ValueIndex, FromLuaProxy, ApplyLua, ReflectPathElem, ReflectionError};


use bevy::reflect::{FromReflect};
use bevy::{reflect::{Reflect}};


use tealr::TypeName;
use tealr::mlu::TypedFunction;
use tealr::mlu::mlua::{MetaMethod};
use tealr::mlu::{mlua::{Lua, Value,self, UserData, ToLua,FromLua}, TealData, TealDataMethods};


use paste::paste;


/// Implements custom user data for simple copy types which implement to and from lua
macro_rules! impl_proxyable_by_copy(
    ( $($num_ty:ty),*) => {
        paste! {
            $(
                impl LuaProxyable for $num_ty {
                    fn ref_to_lua< 'lua>(self_: crate::ScriptRef,lua: & 'lua tealr::mlu::mlua::Lua) -> tealr::mlu::mlua::Result<tealr::mlu::mlua::Value< 'lua> >  {
                        self_.get_typed(|self_ : &Self| self_.to_lua(lua))?
                    }
                
                    fn apply_lua< 'lua>(self_: &mut crate::ScriptRef,lua: & 'lua tealr::mlu::mlua::Lua,new_val:tealr::mlu::mlua::Value< 'lua>) -> tealr::mlu::mlua::Result<()>  {
                        self_.set_val(Self::from_lua(new_val,lua)?)?;
                        Ok(())
                    }
                }

                impl <'lua>FromLuaProxy<'lua> for $num_ty {
                    #[inline(always)]
                    fn from_lua_proxy(new_value: Value<'lua>, lua: &'lua Lua) -> tealr::mlu::mlua::Result<Self> {
                        Self::from_lua(new_value,lua)
                    }
                }

                impl <'lua>ToLuaProxy<'lua> for $num_ty {
                    #[inline(always)]
                    fn to_lua_proxy(self, lua: &'lua Lua) -> tealr::mlu::mlua::Result<Value<'lua>> {
                        self.to_lua(lua)
                    }
                }
            )*
        }
    }  
);


impl_proxyable_by_copy!(bool);
impl_proxyable_by_copy!(f32,f64);
impl_proxyable_by_copy!(i8,i16,i32,i64,i128,isize);
impl_proxyable_by_copy!(u8,u16,u32,u64,u128,usize);

impl LuaProxyable for String {
    fn ref_to_lua<'lua>(self_: ScriptRef,lua: & 'lua Lua) -> mlua::Result<Value< 'lua> >  {
        self_.get_typed(|self_ : &String| self_.as_str().to_lua(lua))?
    }

    fn apply_lua<'lua>(self_: &mut ScriptRef,lua: & 'lua Lua,new_val:Value< 'lua>) -> mlua::Result<()>  {
        self_.get_mut_typed(|self_| Ok(*self_ = Self::from_lua(new_val,lua)?))?
    }
}

impl <'lua>FromLuaProxy<'lua> for String {
    fn from_lua_proxy(new_val : Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
        Self::from_lua(new_val, lua)
    }
}

impl <'lua>ToLuaProxy<'lua> for String {
    fn to_lua_proxy(self, lua: &'lua Lua) -> mlua::Result<Value<'lua>> {
        self.to_lua(lua)
    }
}

impl <T : LuaProxyable + Reflect + for<'a>FromLuaProxy<'a> + Clone>LuaProxyable for Option<T>{
    fn ref_to_lua< 'lua>(self_: ScriptRef,lua: & 'lua Lua) -> mlua::Result<Value< 'lua>>  {
        self_.get_typed(|s : &Option<T>| match  s {
            Some(_) => T::ref_to_lua(self_.sub_ref(ReflectPathElem::SubReflection{
                    label: "as_ref",
                    get: |ref_| 
                        ref_.downcast_ref::<Option<T>>()
                            .ok_or_else(|| ReflectionError::CannotDowncast{from:ref_.type_name().to_owned().into(),to:stringify!(Option<T>).into()})?
                            .as_ref()
                            .map(|t| t as &dyn Reflect)
                            .ok_or_else(|| ReflectionError::Other("Stale reference to Option. Cannot sub reflect.".to_owned())),
                    get_mut: |ref_| 
                        ref_.downcast_mut::<Option<T>>()
                            // TODO: there is some weird borrow checker fuckery going on here
                            // i tried having from: ref_.type_name().to_owned().into() instead of "Reflect"
                            // and lying this out in an if let expression, but nothing will satisfy the borrow checker here, so leaving this for now
                            .ok_or_else(|| ReflectionError::CannotDowncast{from:"Reflect".into(),to:stringify!(Option<T>).into()})?
                            .as_mut()
                            .map(|t| t as &mut dyn Reflect)
                            .ok_or_else(|| ReflectionError::Other("Stale reference to Option. Cannot sub reflect.".to_owned())),
                    })
                ,lua ),
            None => Ok(Value::Nil),
        })?
    }

    fn apply_lua< 'lua>(self_: &mut ScriptRef,lua: & 'lua Lua,new_val:Value< 'lua>) -> mlua::Result<()>  {
        if let Value::Nil = new_val {
            self_.get_mut_typed(|s : &mut Option<T>| Ok(*s = None))?
        } else {
            // we need to do this in two passes, first 
            // ensure that the target type is the 'some' variant to allow a sub reference
            match self_.get_mut_typed(|s : &mut Option<T>| {
                if s.is_none() {
                    *s = Some(T::from_lua_proxy(new_val.clone(),lua)?);
                    Ok::<_,mlua::Error>(true)
                } else {
                    Ok(false)
                }
            })?{
                Ok(true) => return Ok(()),
                Ok(false) => {},
                Err(e) => return Err(e),
            }

            T::apply_lua(
                &mut self_.sub_ref(ReflectPathElem::SubReflection { 
                    label: "",
                    get: |ref_| 
                        ref_.downcast_ref::<Option<T>>()
                            .ok_or_else(|| ReflectionError::CannotDowncast{from:ref_.type_name().to_owned().into(),to:stringify!(Option<T>).into()})?
                            .as_ref()
                            .map(|t| t as &dyn Reflect)
                            .ok_or_else(|| ReflectionError::Other("Stale reference to Option. Cannot sub reflect.".to_owned())),
                    get_mut: |ref_| 
                        if ref_.is::<Option<T>>(){
                            ref_.downcast_mut::<Option<T>>()
                                .unwrap()
                                .as_mut()
                                .map(|t| t as &mut dyn Reflect)
                                .ok_or_else(|| ReflectionError::Other("Stale reference to Option. Cannot sub reflect.".to_owned()))
                        } else {
                            Err(ReflectionError::CannotDowncast{from:ref_.type_name().to_owned().into(),to:stringify!(Option<T>).into()})
                        }
                })
                ,lua, 
                new_val)
            
        }
    }
}

impl <'lua,T : for<'a>FromLuaProxy<'a>>FromLuaProxy<'lua> for Option<T> {
    fn from_lua_proxy(new_val : Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
        if let Value::Nil = new_val {
            Ok(None)
        } else {
            T::from_lua_proxy(new_val, lua).map(Option::Some)
        }
    }
}

impl <'lua,T : for<'a>ToLuaProxy<'a>>ToLuaProxy<'lua> for Option<T> {
    fn to_lua_proxy(self, lua: &'lua Lua) -> mlua::Result<Value<'lua>> {
        match self {
            Some(v) => v.to_lua_proxy(lua),
            None => Ok(Value::Nil),
        }
    }
}

/// A reference to a rust vec, does not need an owned variant since 
/// lua can natively represent lists of things
pub struct LuaVec<T>{
    ref_: ScriptRef,
    _ph: PhantomData<T>
}

impl <T>LuaVec<T> {
    pub fn new_ref(ref_ : ScriptRef) -> Self{
        Self {
            ref_,
            _ph: PhantomData,
        }
    }
}

impl <T : FromReflect>TypeName for LuaVec<T> {
    fn get_type_parts() -> Cow<'static, [tealr::NamePart]> {
        Default::default()
    }
}


impl<T: FromReflect + LuaProxyable + for<'a>FromLuaProxy<'a> + for<'a>ToLuaProxy<'a>> UserData for LuaVec<T>
{
    fn add_methods<'lua, M: ::tealr::mlu::mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
        let mut x = ::tealr::mlu::UserDataWrapper::from_user_data_methods(methods);
        <Self as ::tealr::mlu::TealData>::add_methods(&mut x);
    }
    fn add_fields<'lua, F: ::tealr::mlu::mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        let mut wrapper = ::tealr::mlu::UserDataWrapper::from_user_data_fields(fields);
        <Self as ::tealr::mlu::TealData>::add_fields(&mut wrapper)
    }
}


impl <T : FromReflect + LuaProxyable + for<'a> FromLuaProxy<'a> + for<'a>ToLuaProxy<'a> >TealData for LuaVec<T> {
    fn add_methods<'lua, M: TealDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |_,s,index : usize|{
           Ok(s.ref_.index(index))
        });

        methods.add_meta_method_mut(MetaMethod::NewIndex, |ctx,s,(index,value) : (usize, Value)|{
            Ok(s.ref_.index(index).apply_lua(ctx, value)?)
        });

        methods.add_meta_method(MetaMethod::Pairs, |ctx, s, _ : ()|{
            let len = s.ref_.get_typed(|s : &Vec<T>| s.len())?;
            let mut curr_idx = 0;
            let ref_ = s.ref_.clone();
            TypedFunction::from_rust_mut(move |ctx,()|{
                    let o = 
                    if curr_idx < len {
                        (curr_idx.to_lua(ctx)?,ref_.index(curr_idx).to_lua(ctx)?)
                    } else {
                        (Value::Nil,Value::Nil)
                    };
                    curr_idx += 1;
                    Ok(o)
            },ctx)
        });

        methods.add_meta_method(MetaMethod::Len, |_, s, ()| {
            s.ref_.get_typed(|s : &Vec<T>| Ok(s.len()))?
        });

        methods.add_method_mut("push", |ctx,s,v : Value|{
            let new_val = T::from_lua_proxy(v, ctx)?;
            s.ref_.get_mut_typed(|s : &mut Vec<T>| Ok(s.push(new_val)))?
        });

        methods.add_method_mut("pop", |ctx,s,()|{
            s.ref_.get_mut_typed(|s : &mut Vec<T>| {
                s.pop().to_lua_proxy(ctx)
            })?
        });

        methods.add_method_mut("clear", |_, s, ()| {
            s.ref_.get_mut_typed(|s : &mut Vec<T>| {
                Ok(s.clear())
            })?
        });

        methods.add_method_mut("insert", |ctx, s, (idx,v) : (usize,Value<'lua>)| {
            s.ref_.get_mut_typed(|s : &mut Vec<T>| {
                let v = T::from_lua_proxy(v, ctx)?;
                Ok(s.insert(idx,v))
            })?
        });

        methods.add_method_mut("remove", |ctx, s, idx : usize| {
            s.ref_.get_mut_typed(|s : &mut Vec<T>| {
                Ok(s.remove(idx).to_lua_proxy(ctx)?)
            })?
        });

    }
}

impl <T : FromReflect + LuaProxyable + for<'a>FromLuaProxy<'a> + for<'a>ToLuaProxy<'a>>LuaProxyable for Vec<T> {
    fn ref_to_lua<'lua>(self_ : ScriptRef, lua: &'lua Lua) -> mlua::Result<Value<'lua>> {
        LuaVec::<T>::new_ref(self_).to_lua(lua)
    }

    fn apply_lua<'lua>(self_ : &mut ScriptRef, lua: &'lua Lua, new_val: Value<'lua>) -> mlua::Result<()> {
        // general idea TODO: in case two script refs are identical we don't need to go through this method at all

        match &new_val {
            Value::UserData(ud) => {
                let lua_vec = ud.borrow::<LuaVec<T>>()?;
                self_.apply(&lua_vec.ref_)?;
            },
            Value::Table(table) => {    

                let target_len = self_.get_typed(|s : &Vec<T>| s.len())?;
                // there is also another case to consider, Vec has a lua representation available as well (table)
                // if we receive one of those, we should also apply it
                for entry in table.clone().pairs::<usize,Value>() {
                    let (lua_idx,v) = entry?;
                    let idx = lua_idx - 1;
                    if lua_idx > target_len {
                        // here we don't need to do anything special just use LuaProxyable impl
                        T::apply_lua(&mut self_.index(idx), lua, v)?;
                    } else {
                        // here we don't have anything to apply this to
                        // use FromLua impl
                        self_.get_mut_typed(|s : &mut Vec<T>| 
                            Ok::<_,mlua::Error>(s[idx] = T::from_lua_proxy(v, lua)?)
                        )??;
                    }
                }
            }
            _ => return Err(mlua::Error::FromLuaConversionError { 
                from: new_val.type_name(), 
                to: "userdata or table", 
                message: Some("LuaVec can only be assigned with itself or a table".to_owned()) 
            })
        }

        Ok(())
    }
} 

impl <'lua,T : for<'a>FromLuaProxy<'a> + for<'a>ToLuaProxy<'a> + Clone + FromReflect + LuaProxyable>FromLuaProxy<'lua> for Vec<T> {
    fn from_lua_proxy(new_val : Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
        match &new_val {
            Value::UserData(ud) => {
                let lua_vec = ud.borrow::<LuaVec<T>>()?;
                lua_vec.ref_.get_typed(|s : &Vec<T>| Ok(s.clone()))?
            },
            Value::Table(table) => {    

                let mut out = Vec::default(); 
                // there is also another case to consider, Vec has a lua representation available as well (table)
                // if we receive one of those, we should clone it one by one
                for entry in table.clone().pairs::<usize,Value>() {
                    let (_,v) = entry?;

                    out.push(T::from_lua_proxy(v, lua)?);
                }

                Ok(out)
            }
            _ => return Err(mlua::Error::FromLuaConversionError { 
                from: new_val.type_name(), 
                to: "userdata or table", 
                message: Some("LuaVec can only be assigned with itself or a table".to_owned()) 
            })
        }
    }
}

impl <'lua,T : for<'a>ToLuaProxy<'a> + Clone + FromReflect + LuaProxyable>ToLuaProxy<'lua> for Vec<T> {
    fn to_lua_proxy(self, lua: &'lua Lua) -> mlua::Result<Value<'lua>> {

        let proxies = lua.create_table()?;
        for (idx,elem) in self.into_iter().enumerate() {
            proxies.raw_set(idx, elem.to_lua_proxy(lua)?)?;
        }

        proxies.to_lua(lua)
    }
}