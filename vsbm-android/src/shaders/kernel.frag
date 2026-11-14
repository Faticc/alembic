// Та же формула, что на странице (сферические координаты, степень 8),
// но без тригонометрии: (z + i*rho)^8 и (x + i*y)^8 считаются тремя
// возведениями комплексного числа в квадрат.
//   b^8*cos(8*theta) = Re (z + i*rho)^8,   b^8*sin(8*theta) = Im (z + i*rho)^8
//   rho^8*cos(8*phi) = Re (x + i*y)^8,     rho^8*sin(8*phi) = Im (x + i*y)^8
vec2 pow8(vec2 z){
   z=vec2(z.x*z.x-z.y*z.y,2.0*z.x*z.y);
   z=vec2(z.x*z.x-z.y*z.y,2.0*z.x*z.y);
   return vec2(z.x*z.x-z.y*z.y,2.0*z.x*z.y);
}
float kernal(vec3 ver){
   vec3 a;
   float b,rho2,rho8;
   vec2 t,p;
   a=ver;
   for(int i=0;i<5;i++){
       rho2=a.x*a.x+a.y*a.y;
       b=rho2+a.z*a.z;
       b=b*b;
       b=b*b;
       t=pow8(vec2(a.z,sqrt(rho2)));
       if(rho2>0.0){
           rho8=rho2*rho2;
           p=pow8(a.xy)*(t.y/(rho8*rho8));
       }
       else{
           p=vec2(t.y,0.0);
       }
       a=vec3(p.x,p.y,t.x)+ver;
       if(b>6.0){
           break;
       }
   }
   return 4.0-a.x*a.x-a.y*a.y-a.z*a.z;
}
